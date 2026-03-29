use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    sync::{Mutex, mpsc},
    time::sleep,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use super::accounts::AccountsService;
use super::codex::{
    ManagedCodexInstallation as CodexInstallation, ensure_managed_codex,
    managed_codex_needs_install, pinned_codex_version,
};
use super::settings::SettingsService;

const LONG_CONTEXT_WINDOW: i64 = 1_000_000;
const LONG_CONTEXT_AUTO_COMPACT_LIMIT: i64 = 950_000;
const INBOX_DEVELOPER_INSTRUCTIONS: &str = "When you have a useful question, uncertainty, or optional decision that would help but does not need to block progress, add a standalone line to your next assistant message in exactly this form: [[inbox-question]] <question>. Then continue with your best assumption. Keep inbox questions short and concrete. Only use blocking request_user_input when you truly cannot proceed safely or correctly without the user's answer.";

#[derive(Default)]
pub struct AppServerService {
    inner: Arc<Mutex<Option<RunningAppServer>>>,
    data_dir: PathBuf,
}

struct RunningAppServer {
    account_id: String,
    ws_url: String,
    port: u16,
    codex_home: PathBuf,
    long_context: bool,
    agent_max_threads: usize,
    agent_max_depth: i32,
    child: Child,
}

#[derive(Debug, Clone)]
struct AppServerConnection {
    account_id: String,
    ws_url: String,
    codex_home: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppServerLaunchOptions {
    long_context: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AppServerSettings {
    agent_max_threads: usize,
    agent_max_depth: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatStreamEvent {
    CodexRuntimeDialog {
        message: Option<String>,
    },
    ThreadReady {
        thread_id: String,
    },
    TurnStarted {
        turn_id: String,
    },
    Status {
        message: String,
    },
    Activity {
        item_id: String,
        title: String,
        detail: String,
        agent_label: Option<String>,
        complete: bool,
    },
    AgentThread {
        thread_id: String,
        label: String,
    },
    CollabTool {
        item_id: String,
        title: String,
        detail: String,
        agent_label: Option<String>,
        tool: String,
        sender_thread_id: String,
        receiver_thread_ids: Vec<String>,
        model: Option<String>,
        reasoning_effort: Option<String>,
        agent_states: Vec<CollabAgentStateInfo>,
        complete: bool,
    },
    TokenUsage {
        context_tokens: u64,
        session_total_tokens: u64,
        context_window: u64,
    },
    AssistantDelta {
        item_id: String,
        title: Option<String>,
        delta: String,
    },
    ReasoningDelta {
        item_id: String,
        title: Option<String>,
        delta: String,
    },
    CommandStarted {
        item_id: String,
        title: Option<String>,
        command: String,
    },
    CommandDelta {
        item_id: String,
        delta: String,
    },
    ItemDone {
        item_id: String,
    },
    Error {
        message: String,
    },
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollabAgentStateInfo {
    pub thread_id: String,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatTurnSettings {
    pub model: String,
    pub effort: Option<String>,
    pub service_tier: Option<String>,
    pub long_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChatAttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatAttachment {
    pub name: String,
    pub path: String,
    pub kind: ChatAttachmentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatModelInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub default_effort: Option<String>,
    pub supported_efforts: Vec<ChatReasoningEffortInfo>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatReasoningEffortInfo {
    pub effort: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeAuthFile {
    tokens: Option<RuntimeAuthTokens>,
}

#[derive(Debug, Deserialize)]
struct RuntimeAuthTokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

impl AppServerService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            inner: Arc::default(),
            data_dir,
        }
    }

    pub async fn stream_chat_prompt(
        &self,
        accounts: Arc<AccountsService>,
        thread_id: Option<String>,
        prompt: String,
        attachments: Vec<ChatAttachment>,
        settings: ChatTurnSettings,
    ) -> Result<mpsc::UnboundedReceiver<ChatStreamEvent>, String> {
        let cwd = self.load_workspace_path()?;
        let launch_options = AppServerLaunchOptions {
            long_context: settings.long_context,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let runtime = self.clone_connection_service();

        tokio::spawn(async move {
            if managed_codex_needs_install(&runtime.data_dir).unwrap_or(false) {
                let _ = tx.send(ChatStreamEvent::CodexRuntimeDialog {
                    message: Some(format!(
                        "Fetching Codex {} for this app...",
                        pinned_codex_version()
                    )),
                });
            }

            let connection = match runtime
                .ensure_running_connection(&accounts, launch_options)
                .await
            {
                Ok(connection) => {
                    let _ = tx.send(ChatStreamEvent::CodexRuntimeDialog { message: None });
                    connection
                }
                Err(error) => {
                    let _ = tx.send(ChatStreamEvent::CodexRuntimeDialog { message: None });
                    let _ = tx.send(ChatStreamEvent::Error { message: error });
                    let _ = tx.send(ChatStreamEvent::Completed);
                    return;
                }
            };

            if let Err(error) = run_chat_turn_with_failover(
                runtime,
                accounts,
                connection,
                thread_id,
                prompt,
                attachments,
                settings,
                cwd,
                tx.clone(),
            )
            .await
            {
                let _ = tx.send(ChatStreamEvent::CodexRuntimeDialog { message: None });
                let _ = tx.send(ChatStreamEvent::Error { message: error });
            }
            let _ = tx.send(ChatStreamEvent::CodexRuntimeDialog { message: None });
            let _ = tx.send(ChatStreamEvent::Completed);
        });

        Ok(rx)
    }

    pub async fn list_models(
        &self,
        accounts: Arc<AccountsService>,
    ) -> Result<Vec<ChatModelInfo>, String> {
        let connection = self
            .ensure_running_connection(
                &accounts,
                AppServerLaunchOptions {
                    long_context: false,
                },
            )
            .await?;
        let mut rpc =
            AppServerRpcClient::connect(&connection.ws_url, &connection.codex_home).await?;
        rpc.initialize().await?;
        rpc.list_models().await
    }

    pub async fn interrupt_turn(&self, thread_id: &str, turn_id: &str) -> Result<(), String> {
        let connection = {
            let mut guard = self.inner.lock().await;
            let Some(running) = guard.as_mut() else {
                return Err("No active Codex session is running.".to_string());
            };
            if running
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                *guard = None;
                return Err("Codex app-server is not running.".to_string());
            }
            running.connection()
        };

        let mut rpc =
            AppServerRpcClient::connect(&connection.ws_url, &connection.codex_home).await?;
        rpc.initialize().await?;
        rpc.interrupt_turn(thread_id, turn_id).await
    }

    pub async fn steer_turn(
        &self,
        thread_id: &str,
        turn_id: &str,
        prompt: String,
        attachments: Vec<ChatAttachment>,
    ) -> Result<(), String> {
        let connection = {
            let mut guard = self.inner.lock().await;
            let Some(running) = guard.as_mut() else {
                return Err("No active Codex session is running.".to_string());
            };
            if running
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                *guard = None;
                return Err("Codex app-server is not running.".to_string());
            }
            running.connection()
        };

        let mut rpc =
            AppServerRpcClient::connect(&connection.ws_url, &connection.codex_home).await?;
        rpc.initialize().await?;
        rpc.steer_turn(thread_id, turn_id, &prompt, &attachments)
            .await
    }

    pub async fn restart(&self) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        if let Some(running) = guard.as_mut() {
            let _ = running.child.start_kill();
        }
        *guard = None;
        Ok(())
    }

    async fn ensure_running_connection(
        &self,
        accounts: &AccountsService,
        launch_options: AppServerLaunchOptions,
    ) -> Result<AppServerConnection, String> {
        let mut guard = self.inner.lock().await;
        let selected_account = accounts.resolve_runtime_account().await?;
        let app_settings = self.load_app_server_settings()?;

        if let Some(running) = guard.as_mut() {
            if running
                .child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                *guard = None;
            } else if selected_account
                .as_ref()
                .is_some_and(|selected| selected.id != running.account_id)
            {
                let _ = running.child.start_kill();
                *guard = None;
            } else if running.long_context != launch_options.long_context {
                let _ = running.child.start_kill();
                *guard = None;
            } else if running.agent_max_threads != app_settings.agent_max_threads
                || running.agent_max_depth != app_settings.agent_max_depth
            {
                let _ = running.child.start_kill();
                *guard = None;
            } else if app_server_is_ready(running.port).await {
                return Ok(running.connection());
            } else {
                let _ = running.child.start_kill();
                *guard = None;
            }
        }

        let installation = self.ensure_codex_installation().await?;
        let port = reserve_local_port()?;
        let ws_url = format!("ws://127.0.0.1:{port}");

        let selected_account = selected_account.ok_or_else(|| {
            "No linked Codex account found. Add an API key or link a ChatGPT account first."
                .to_string()
        })?;

        let mut command = Command::new(&installation.codex_bin);
        command
            .arg("-c")
            .arg(format!(
                "agents.max_threads={}",
                app_settings.agent_max_threads
            ))
            .arg("-c")
            .arg(format!("agent_max_depth={}", app_settings.agent_max_depth));
        if launch_options.long_context {
            command
                .arg("-c")
                .arg(format!("model_context_window={LONG_CONTEXT_WINDOW}"))
                .arg("-c")
                .arg(format!(
                    "model_auto_compact_token_limit={LONG_CONTEXT_AUTO_COMPACT_LIMIT}"
                ));
        }

        let mut child = command
            .arg("app-server")
            .arg("--listen")
            .arg(&ws_url)
            .env("CODEX_HOME", &selected_account.codex_home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("failed to start Codex app-server: {error}"))?;

        wait_for_app_server_ready(&mut child, port).await?;

        *guard = Some(RunningAppServer {
            account_id: selected_account.id.clone(),
            ws_url: ws_url.clone(),
            port,
            codex_home: selected_account.codex_home.clone(),
            long_context: launch_options.long_context,
            agent_max_threads: app_settings.agent_max_threads,
            agent_max_depth: app_settings.agent_max_depth,
            child,
        });

        Ok(AppServerConnection {
            account_id: selected_account.id,
            ws_url,
            codex_home: selected_account.codex_home,
        })
    }

    async fn restart_with_best_account(
        &self,
        accounts: &AccountsService,
        previous_account_id: &str,
        launch_options: AppServerLaunchOptions,
    ) -> Result<Option<AppServerConnection>, String> {
        let _ = accounts.refresh_all_account_limits().await?;

        {
            let mut guard = self.inner.lock().await;
            if let Some(running) = guard.as_mut() {
                let _ = running.child.start_kill();
            }
            *guard = None;
        }

        let connection = self
            .ensure_running_connection(accounts, launch_options)
            .await?;
        if connection.account_id == previous_account_id {
            return Ok(None);
        }

        Ok(Some(connection))
    }

    async fn ensure_codex_installation(&self) -> Result<CodexInstallation, String> {
        ensure_managed_codex(&self.data_dir).await
    }

    fn load_app_server_settings(&self) -> Result<AppServerSettings, String> {
        let settings = SettingsService::new(self.data_dir.clone()).load()?;
        Ok(AppServerSettings {
            agent_max_threads: settings.agent_max_threads.max(1),
            agent_max_depth: settings.agent_max_depth.max(1),
        })
    }

    fn load_workspace_path(&self) -> Result<PathBuf, String> {
        SettingsService::new(self.data_dir.clone()).current_workspace_path()
    }

    fn clone_connection_service(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            data_dir: self.data_dir.clone(),
        }
    }
}

impl RunningAppServer {
    fn connection(&self) -> AppServerConnection {
        AppServerConnection {
            account_id: self.account_id.clone(),
            ws_url: self.ws_url.clone(),
            codex_home: self.codex_home.clone(),
        }
    }
}

type AppServerSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct AppServerRpcClient {
    socket: AppServerSocket,
    next_id: u64,
    codex_home: PathBuf,
    buffered_messages: VecDeque<IncomingMessage>,
}

enum IncomingMessage {
    Response {
        id: u64,
        payload: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: u64,
        method: String,
        params: Value,
    },
}

fn web_search_detail_from_item(item: &Value) -> String {
    let query = item
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let Some(action) = item.get("action") else {
        return query.to_string();
    };

    match action.get("type").and_then(Value::as_str) {
        Some("search") => action
            .get("query")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                action
                    .get("queries")
                    .and_then(Value::as_array)
                    .and_then(|queries| queries.first())
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| query.to_string()),
        Some("openPage") => action
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some("findInPage") => {
            let pattern = action.get("pattern").and_then(Value::as_str);
            let url = action.get("url").and_then(Value::as_str);
            match (pattern, url) {
                (Some(pattern), Some(url)) => format!("'{pattern}' in {url}"),
                (Some(pattern), None) => format!("'{pattern}'"),
                (None, Some(url)) => url.to_string(),
                (None, None) => query.to_string(),
            }
        }
        _ => query.to_string(),
    }
}

fn collab_tool_title(tool: &str, receiver_count: usize, complete: bool) -> String {
    match (tool, complete) {
        ("spawnAgent", false) => "Spawning agent".to_string(),
        ("spawnAgent", true) => "Spawned agent".to_string(),
        ("sendInput", false) => "Sending input to agent".to_string(),
        ("sendInput", true) => "Sent input to agent".to_string(),
        ("resumeAgent", false) => "Resuming agent".to_string(),
        ("resumeAgent", true) => "Resumed agent".to_string(),
        ("wait", false) if receiver_count == 1 => "Waiting for agent".to_string(),
        ("wait", false) => format!("Waiting for {receiver_count} agents"),
        ("wait", true) => "Finished waiting".to_string(),
        ("closeAgent", false) => "Closing agent".to_string(),
        ("closeAgent", true) => "Closed agent".to_string(),
        (_, false) => "Running swarm tool".to_string(),
        _ => "Completed swarm tool".to_string(),
    }
}

fn collab_tool_detail_from_item(item: &Value) -> String {
    let prompt = item
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);
    let model = item.get("model").and_then(Value::as_str);
    let reasoning = item.get("reasoningEffort").and_then(Value::as_str);
    let receiver_count = item
        .get("receiverThreadIds")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);

    let mut parts = Vec::new();
    if let Some(prompt) = prompt {
        parts.push(prompt);
    }
    if let Some(model) = model.filter(|value| !value.is_empty()) {
        parts.push(model.to_string());
    }
    if let Some(reasoning) = reasoning.filter(|value| !value.is_empty()) {
        parts.push(reasoning.to_string());
    }
    if receiver_count > 1 {
        parts.push(format!("{receiver_count} targets"));
    }

    parts.join(" · ")
}

fn collab_tool_from_item(item: &Value, complete: bool) -> Option<ChatStreamEvent> {
    if item.get("type").and_then(Value::as_str)? != "collabAgentToolCall" {
        return None;
    }

    let item_id = item.get("id").and_then(Value::as_str)?.to_string();
    let tool = item.get("tool").and_then(Value::as_str)?.to_string();
    let sender_thread_id = item
        .get("senderThreadId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let receiver_thread_ids = item
        .get("receiverThreadIds")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let agent_states = item
        .get("agentsStates")
        .and_then(Value::as_object)
        .map(|states| {
            states
                .iter()
                .map(|(thread_id, state)| CollabAgentStateInfo {
                    thread_id: thread_id.clone(),
                    status: state
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("running")
                        .to_string(),
                    message: state
                        .get("message")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let detail = collab_tool_detail_from_item(item);

    Some(ChatStreamEvent::CollabTool {
        item_id,
        title: collab_tool_title(&tool, receiver_thread_ids.len(), complete),
        detail,
        agent_label: None,
        tool,
        sender_thread_id,
        receiver_thread_ids,
        model: item
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        reasoning_effort: item
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        agent_states,
        complete,
    })
}

fn activity_started_from_item(item: &Value) -> Option<(String, String, String)> {
    let item_id = item.get("id").and_then(Value::as_str)?.to_string();
    match item.get("type").and_then(Value::as_str)? {
        "webSearch" => Some((
            item_id,
            "Searching the web".to_string(),
            web_search_detail_from_item(item),
        )),
        _ => None,
    }
}

fn activity_completed_from_item(item: &Value) -> Option<(String, String, String)> {
    let item_id = item.get("id").and_then(Value::as_str)?.to_string();
    match item.get("type").and_then(Value::as_str)? {
        "webSearch" => Some((
            item_id,
            "Searched the web".to_string(),
            web_search_detail_from_item(item),
        )),
        _ => None,
    }
}

fn thread_matches(params: &Value, watched_thread_ids: &HashSet<String>) -> bool {
    params
        .get("threadId")
        .and_then(Value::as_str)
        .is_some_and(|thread_id| watched_thread_ids.contains(thread_id))
}

fn thread_label_for_params(
    params: &Value,
    root_thread_id: &str,
    thread_labels: &mut HashMap<String, String>,
    next_agent_label: &mut usize,
) -> Option<String> {
    let thread_id = params.get("threadId").and_then(Value::as_str)?;
    if thread_id == root_thread_id {
        return None;
    }

    Some(
        thread_labels
            .entry(thread_id.to_string())
            .or_insert_with(|| {
                let label = format!("Agent {next_agent_label}");
                *next_agent_label += 1;
                label
            })
            .clone(),
    )
}

fn prefix_title(prefix: Option<String>, title: String) -> String {
    match prefix {
        Some(prefix) => format!("{prefix} · {title}"),
        None => title,
    }
}

fn enrich_collab_tool_event(
    event: ChatStreamEvent,
    thread_labels: &HashMap<String, String>,
    pending_collab_labels: &HashMap<String, String>,
) -> ChatStreamEvent {
    match event {
        ChatStreamEvent::CollabTool {
            item_id,
            title,
            detail,
            tool,
            sender_thread_id,
            receiver_thread_ids,
            model,
            reasoning_effort,
            agent_states,
            complete,
            ..
        } => {
            let agent_label = receiver_thread_ids
                .first()
                .and_then(|thread_id| thread_labels.get(thread_id))
                .cloned()
                .or_else(|| pending_collab_labels.get(&item_id).cloned());

            ChatStreamEvent::CollabTool {
                item_id,
                title,
                detail,
                agent_label,
                tool,
                sender_thread_id,
                receiver_thread_ids,
                model,
                reasoning_effort,
                agent_states,
                complete,
            }
        }
        other => other,
    }
}

async fn run_chat_turn_stream(
    connection: AppServerConnection,
    thread_id: Option<String>,
    prompt: String,
    attachments: Vec<ChatAttachment>,
    settings: ChatTurnSettings,
    cwd: PathBuf,
    tx: mpsc::UnboundedSender<ChatStreamEvent>,
) -> Result<(), String> {
    let mut saw_assistant_delta = false;
    let mut rpc = AppServerRpcClient::connect(&connection.ws_url, &connection.codex_home).await?;
    rpc.initialize().await?;

    let thread_id = match thread_id {
        Some(existing) => match rpc
            .resume_thread(&existing, settings.service_tier.as_deref())
            .await
        {
            Ok(()) => match rpc
                .start_turn(&existing, &prompt, &attachments, &settings)
                .await
            {
                Ok(turn_id) => {
                    let _ = tx.send(ChatStreamEvent::TurnStarted { turn_id });
                    existing
                }
                Err(error) if error_mentions_missing_thread(&error) => {
                    let fresh = rpc
                        .start_thread(&cwd, &settings.model, settings.service_tier.as_deref())
                        .await?;
                    let turn_id = rpc
                        .start_turn(&fresh, &prompt, &attachments, &settings)
                        .await?;
                    let _ = tx.send(ChatStreamEvent::TurnStarted { turn_id });
                    fresh
                }
                Err(error) => return Err(error),
            },
            Err(error) if error_mentions_missing_thread(&error) => {
                let fresh = rpc
                    .start_thread(&cwd, &settings.model, settings.service_tier.as_deref())
                    .await?;
                let turn_id = rpc
                    .start_turn(&fresh, &prompt, &attachments, &settings)
                    .await?;
                let _ = tx.send(ChatStreamEvent::TurnStarted { turn_id });
                fresh
            }
            Err(error) => return Err(error),
        },
        None => {
            let fresh = rpc
                .start_thread(&cwd, &settings.model, settings.service_tier.as_deref())
                .await?;
            let turn_id = rpc
                .start_turn(&fresh, &prompt, &attachments, &settings)
                .await?;
            let _ = tx.send(ChatStreamEvent::TurnStarted { turn_id });
            fresh
        }
    };

    let _ = tx.send(ChatStreamEvent::ThreadReady {
        thread_id: thread_id.clone(),
    });

    rpc.wait_for_turn_completion(&thread_id, &tx, &mut saw_assistant_delta)
        .await?;

    if !saw_assistant_delta {
        let thread = rpc.read_thread(&thread_id).await?;
        if let Some(reply) = extract_latest_assistant_reply(&thread) {
            let _ = tx.send(ChatStreamEvent::AssistantDelta {
                item_id: format!("assistant:{thread_id}"),
                title: None,
                delta: reply,
            });
            let _ = tx.send(ChatStreamEvent::ItemDone {
                item_id: format!("assistant:{thread_id}"),
            });
        }
    }

    Ok(())
}

async fn run_chat_turn_with_failover(
    runtime: AppServerService,
    accounts: Arc<AccountsService>,
    connection: AppServerConnection,
    thread_id: Option<String>,
    prompt: String,
    attachments: Vec<ChatAttachment>,
    settings: ChatTurnSettings,
    cwd: PathBuf,
    tx: mpsc::UnboundedSender<ChatStreamEvent>,
) -> Result<(), String> {
    let _ = tx.send(ChatStreamEvent::Status {
        message: "Thinking...".to_string(),
    });

    match run_chat_turn_stream(
        connection.clone(),
        thread_id,
        prompt.clone(),
        attachments.clone(),
        settings.clone(),
        cwd.clone(),
        tx.clone(),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) if should_failover_account(&error) => {
            if let Some(next_connection) = runtime
                .restart_with_best_account(
                    &accounts,
                    &connection.account_id,
                    AppServerLaunchOptions {
                        long_context: settings.long_context,
                    },
                )
                .await?
            {
                let _ = tx.send(ChatStreamEvent::Status {
                    message: "Switching to another available account...".to_string(),
                });
                return run_chat_turn_stream(
                    next_connection,
                    None,
                    prompt,
                    attachments,
                    settings,
                    cwd,
                    tx,
                )
                .await;
            }

            Err(error)
        }
        Err(error) => Err(error),
    }
}

impl AppServerRpcClient {
    async fn connect(ws_url: &str, codex_home: &Path) -> Result<Self, String> {
        let (socket, _) = connect_async(ws_url)
            .await
            .map_err(|error| format!("failed to connect to Codex app-server: {error}"))?;

        Ok(Self {
            socket,
            next_id: 1,
            codex_home: codex_home.to_path_buf(),
            buffered_messages: VecDeque::new(),
        })
    }

    async fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "miawk",
                    "title": "MIAWK",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "experimentalApi": true,
                }
            }),
        )
        .await?;

        self.notify("initialized", Value::Null).await
    }

    async fn start_thread(
        &mut self,
        cwd: &Path,
        model: &str,
        service_tier: Option<&str>,
    ) -> Result<String, String> {
        let response = self
            .request(
                "thread/start",
                json!({
                    "model": model,
                    "serviceTier": service_tier,
                    "cwd": cwd.to_string_lossy(),
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "developerInstructions": INBOX_DEVELOPER_INSTRUCTIONS,
                    "personality": "friendly",
                }),
            )
            .await?;

        response
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "Codex app-server did not return a thread id.".to_string())
    }

    async fn resume_thread(
        &mut self,
        thread_id: &str,
        service_tier: Option<&str>,
    ) -> Result<(), String> {
        self.request(
            "thread/resume",
            json!({
                "threadId": thread_id,
                "serviceTier": service_tier,
                "developerInstructions": INBOX_DEVELOPER_INSTRUCTIONS,
                "personality": "friendly",
            }),
        )
        .await?;

        Ok(())
    }

    async fn start_turn(
        &mut self,
        thread_id: &str,
        prompt: &str,
        attachments: &[ChatAttachment],
        settings: &ChatTurnSettings,
    ) -> Result<String, String> {
        let mut input = vec![json!({
            "type": "text",
            "text": prompt,
            "text_elements": [],
        })];

        for attachment in attachments {
            match attachment.kind {
                ChatAttachmentKind::Image => input.push(json!({
                    "type": "localImage",
                    "path": attachment.path,
                })),
                ChatAttachmentKind::File => input.push(json!({
                    "type": "text",
                    "text": format!("Attached file: {}", attachment.path),
                    "text_elements": [],
                })),
            }
        }

        let response = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "model": settings.model,
                    "effort": settings.effort,
                    "serviceTier": settings.service_tier,
                    "collaborationMode": {
                        "mode": "default",
                        "settings": {
                            "model": settings.model,
                            "reasoning_effort": settings.effort,
                            "developer_instructions": Value::Null,
                        }
                    },
                    "input": input,
                }),
            )
            .await?;

        response
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "Codex app-server did not return a turn id.".to_string())
    }

    async fn interrupt_turn(&mut self, thread_id: &str, turn_id: &str) -> Result<(), String> {
        self.request(
            "turn/interrupt",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
            }),
        )
        .await?;
        Ok(())
    }

    async fn steer_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        prompt: &str,
        attachments: &[ChatAttachment],
    ) -> Result<(), String> {
        let mut input = vec![json!({
            "type": "text",
            "text": prompt,
            "text_elements": [],
        })];

        for attachment in attachments {
            match attachment.kind {
                ChatAttachmentKind::Image => input.push(json!({
                    "type": "localImage",
                    "path": attachment.path,
                })),
                ChatAttachmentKind::File => input.push(json!({
                    "type": "text",
                    "text": format!("Attached file: {}", attachment.path),
                    "text_elements": [],
                })),
            }
        }

        self.request(
            "turn/steer",
            json!({
                "threadId": thread_id,
                "expectedTurnId": turn_id,
                "input": input,
            }),
        )
        .await?;
        Ok(())
    }

    async fn list_models(&mut self) -> Result<Vec<ChatModelInfo>, String> {
        let response = self
            .request(
                "model/list",
                json!({
                    "limit": 100,
                    "includeHidden": false,
                }),
            )
            .await?;

        let models = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "Codex app-server did not return a model list.".to_string())?
            .iter()
            .filter_map(|entry| {
                let id = entry.get("id").and_then(Value::as_str)?.to_string();
                let display_name = entry
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string();
                let description = entry
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let default_effort = entry
                    .get("defaultReasoningEffort")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let supported_efforts = entry
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .map(|efforts| {
                        efforts
                            .iter()
                            .filter_map(|effort| {
                                let value = effort
                                    .get("reasoningEffort")
                                    .and_then(Value::as_str)?
                                    .to_string();
                                let description = effort
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                Some(ChatReasoningEffortInfo {
                                    effort: value,
                                    description,
                                })
                            })
                            .collect::<Vec<ChatReasoningEffortInfo>>()
                    })
                    .unwrap_or_default();
                let is_default = entry
                    .get("isDefault")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                Some(ChatModelInfo {
                    id,
                    display_name,
                    description,
                    default_effort,
                    supported_efforts,
                    is_default,
                })
            })
            .collect::<Vec<_>>();

        Ok(models)
    }

    async fn read_thread(&mut self, thread_id: &str) -> Result<Value, String> {
        self.request(
            "thread/read",
            json!({
                "threadId": thread_id,
                "includeTurns": true,
            }),
        )
        .await
    }

    async fn wait_for_turn_completion(
        &mut self,
        thread_id: &str,
        tx: &mpsc::UnboundedSender<ChatStreamEvent>,
        saw_assistant_delta: &mut bool,
    ) -> Result<(), String> {
        let mut pending_error = None::<String>;
        let mut watched_thread_ids = HashSet::from([thread_id.to_string()]);
        let mut thread_labels = HashMap::new();
        thread_labels.insert(thread_id.to_string(), "Brain".to_string());
        let mut next_agent_label = 1_usize;
        let mut pending_collab_labels = HashMap::<String, String>::new();

        loop {
            match self.next_message().await? {
                IncomingMessage::Notification { method, params } => match method.as_str() {
                    "thread/started" => {
                        let Some(thread) = params.get("thread") else {
                            continue;
                        };
                        let Some(started_thread_id) = thread.get("id").and_then(Value::as_str)
                        else {
                            continue;
                        };
                        let label = thread
                            .get("agentNickname")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                            .or_else(|| {
                                thread
                                    .get("agentRole")
                                    .and_then(Value::as_str)
                                    .map(|role| role.to_string())
                            })
                            .or_else(|| {
                                if started_thread_id == thread_id {
                                    Some("Brain".to_string())
                                } else {
                                    let label = format!("Agent {next_agent_label}");
                                    next_agent_label += 1;
                                    Some(label)
                                }
                            })
                            .unwrap_or_else(|| "Agent".to_string());
                        watched_thread_ids.insert(started_thread_id.to_string());
                        thread_labels.insert(started_thread_id.to_string(), label.clone());
                        if started_thread_id != thread_id {
                            let _ = tx.send(ChatStreamEvent::AgentThread {
                                thread_id: started_thread_id.to_string(),
                                label,
                            });
                        }
                    }
                    "error" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }
                        if params.get("threadId").and_then(Value::as_str) == Some(thread_id) {
                            pending_error = params
                                .pointer("/error/message")
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                                .or(pending_error);
                        }
                    }
                    "turn/completed" => {
                        if params.get("threadId").and_then(Value::as_str) != Some(thread_id) {
                            continue;
                        }

                        if let Some(message) = params
                            .pointer("/turn/error/message")
                            .and_then(Value::as_str)
                        {
                            return Err(message.to_string());
                        }

                        if let Some(message) = pending_error.take() {
                            return Err(message);
                        }

                        return Ok(());
                    }
                    "item/agentMessage/delta" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }
                        let thread_label = thread_label_for_params(
                            &params,
                            thread_id,
                            &mut thread_labels,
                            &mut next_agent_label,
                        );

                        let item_id = params
                            .get("itemId")
                            .and_then(Value::as_str)
                            .unwrap_or("assistant")
                            .to_string();
                        let delta = params
                            .get("delta")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();

                        if !delta.is_empty() {
                            *saw_assistant_delta = true;
                            let _ = tx.send(ChatStreamEvent::AssistantDelta {
                                item_id,
                                title: thread_label,
                                delta,
                            });
                        }
                    }
                    "thread/tokenUsage/updated" => {
                        if params.get("threadId").and_then(Value::as_str) != Some(thread_id) {
                            continue;
                        }

                        let context_tokens = params
                            .pointer("/tokenUsage/last/totalTokens")
                            .and_then(Value::as_u64);
                        let session_total_tokens = params
                            .pointer("/tokenUsage/total/totalTokens")
                            .and_then(Value::as_u64);
                        let context_window = params
                            .pointer("/tokenUsage/modelContextWindow")
                            .and_then(Value::as_u64);

                        if let (
                            Some(context_tokens),
                            Some(session_total_tokens),
                            Some(context_window),
                        ) = (context_tokens, session_total_tokens, context_window)
                        {
                            let _ = tx.send(ChatStreamEvent::TokenUsage {
                                context_tokens,
                                session_total_tokens,
                                context_window,
                            });
                        }
                    }
                    "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }
                        let thread_label = thread_label_for_params(
                            &params,
                            thread_id,
                            &mut thread_labels,
                            &mut next_agent_label,
                        );

                        let item_id = params
                            .get("itemId")
                            .and_then(Value::as_str)
                            .unwrap_or("reasoning")
                            .to_string();
                        let delta = params
                            .get("delta")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();

                        if !delta.is_empty() {
                            let _ = tx.send(ChatStreamEvent::ReasoningDelta {
                                item_id,
                                title: thread_label,
                                delta,
                            });
                        }
                    }
                    "item/commandExecution/outputDelta" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }

                        let item_id = params
                            .get("itemId")
                            .and_then(Value::as_str)
                            .unwrap_or("command")
                            .to_string();
                        let delta = params
                            .get("delta")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();

                        if !delta.is_empty() {
                            let _ = tx.send(ChatStreamEvent::CommandDelta { item_id, delta });
                        }
                    }
                    "item/started" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }
                        let thread_label = thread_label_for_params(
                            &params,
                            thread_id,
                            &mut thread_labels,
                            &mut next_agent_label,
                        );

                        let Some(item) = params.get("item") else {
                            continue;
                        };
                        if let Some(event) = collab_tool_from_item(item, false) {
                            if let ChatStreamEvent::CollabTool {
                                item_id,
                                tool,
                                receiver_thread_ids,
                                ..
                            } = &event
                            {
                                if tool == "spawnAgent" {
                                    pending_collab_labels.entry(item_id.clone()).or_insert_with(
                                        || {
                                            let label = format!("Agent {next_agent_label}");
                                            next_agent_label += 1;
                                            label
                                        },
                                    );
                                }
                                for receiver in receiver_thread_ids {
                                    watched_thread_ids.insert(receiver.clone());
                                    thread_labels.entry(receiver.clone()).or_insert_with(|| {
                                        let label = format!("Agent {next_agent_label}");
                                        next_agent_label += 1;
                                        label
                                    });
                                }
                            }
                            let _ = tx.send(enrich_collab_tool_event(
                                event,
                                &thread_labels,
                                &pending_collab_labels,
                            ));
                        }
                        if let Some((item_id, title, detail)) = activity_started_from_item(item) {
                            let _ = tx.send(ChatStreamEvent::Activity {
                                item_id,
                                title: prefix_title(thread_label.clone(), title),
                                detail,
                                agent_label: thread_label.clone(),
                                complete: false,
                            });
                        }
                        match item.get("type").and_then(Value::as_str) {
                            Some("commandExecution") => {
                                let item_id = item
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("command")
                                    .to_string();
                                let command = item
                                    .get("command")
                                    .and_then(Value::as_str)
                                    .unwrap_or("command")
                                    .to_string();
                                let _ = tx.send(ChatStreamEvent::CommandStarted {
                                    item_id,
                                    title: thread_label.clone(),
                                    command,
                                });
                            }
                            Some("agentMessage") => {
                                let _ = tx.send(ChatStreamEvent::Status {
                                    message: "Writing response...".to_string(),
                                });
                            }
                            _ => continue,
                        }
                    }
                    "item/completed" => {
                        if !thread_matches(&params, &watched_thread_ids) {
                            continue;
                        }
                        let thread_label = thread_label_for_params(
                            &params,
                            thread_id,
                            &mut thread_labels,
                            &mut next_agent_label,
                        );

                        let Some(item) = params.get("item") else {
                            continue;
                        };
                        let Some(item_type) = item.get("type").and_then(Value::as_str) else {
                            continue;
                        };
                        if let Some(event) = collab_tool_from_item(item, true) {
                            if let ChatStreamEvent::CollabTool {
                                item_id,
                                tool,
                                receiver_thread_ids,
                                ..
                            } = &event
                            {
                                if tool == "spawnAgent" {
                                    let label = pending_collab_labels
                                        .get(item_id)
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            let label = format!("Agent {next_agent_label}");
                                            next_agent_label += 1;
                                            label
                                        });
                                    for receiver in receiver_thread_ids {
                                        thread_labels.insert(receiver.clone(), label.clone());
                                        watched_thread_ids.insert(receiver.clone());
                                        let _ = tx.send(ChatStreamEvent::AgentThread {
                                            thread_id: receiver.clone(),
                                            label: label.clone(),
                                        });
                                    }
                                }
                                for receiver in receiver_thread_ids {
                                    watched_thread_ids.insert(receiver.clone());
                                    thread_labels.entry(receiver.clone()).or_insert_with(|| {
                                        let label = format!("Agent {next_agent_label}");
                                        next_agent_label += 1;
                                        label
                                    });
                                }
                            }
                            let _ = tx.send(enrich_collab_tool_event(
                                event,
                                &thread_labels,
                                &pending_collab_labels,
                            ));
                        }
                        if let Some((item_id, title, detail)) = activity_completed_from_item(item) {
                            let _ = tx.send(ChatStreamEvent::Activity {
                                item_id,
                                title: prefix_title(thread_label.clone(), title),
                                detail,
                                agent_label: thread_label.clone(),
                                complete: true,
                            });
                        }
                        if !matches!(item_type, "agentMessage" | "reasoning" | "commandExecution") {
                            continue;
                        }

                        if item_type == "commandExecution" {
                            if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                                if let Some(command) = item.get("command").and_then(Value::as_str) {
                                    let _ = tx.send(ChatStreamEvent::CommandStarted {
                                        item_id: item_id.to_string(),
                                        title: thread_label.clone(),
                                        command: command.to_string(),
                                    });
                                }

                                if let Some(output) = item
                                    .get("aggregatedOutput")
                                    .and_then(Value::as_str)
                                    .filter(|text| !text.is_empty())
                                {
                                    let _ = tx.send(ChatStreamEvent::CommandDelta {
                                        item_id: item_id.to_string(),
                                        delta: output.to_string(),
                                    });
                                }
                            }
                        }

                        if let Some(item_id) = item.get("id").and_then(Value::as_str) {
                            let _ = tx.send(ChatStreamEvent::ItemDone {
                                item_id: item_id.to_string(),
                            });
                        }
                    }
                    _ => {}
                },
                IncomingMessage::Request { id, method, params } => {
                    self.handle_server_request(id, &method, params).await?;
                }
                IncomingMessage::Response { .. } => {}
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        self.socket
            .send(Message::Text(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": method,
                    "params": params,
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|error| format!("failed to send app-server request {method}: {error}"))?;

        loop {
            match self.read_socket_message().await? {
                IncomingMessage::Response {
                    id: response_id,
                    payload,
                } if response_id == id => {
                    if let Some(error) = payload.get("error") {
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("Codex app-server returned an unknown error.");
                        return Err(message.to_string());
                    }

                    return payload
                        .get("result")
                        .cloned()
                        .ok_or_else(|| format!("app-server response for {method} had no result"));
                }
                IncomingMessage::Request { id, method, params } => {
                    self.handle_server_request(id, &method, params).await?;
                }
                IncomingMessage::Notification { method, params } => {
                    self.buffered_messages
                        .push_back(IncomingMessage::Notification { method, params });
                }
                IncomingMessage::Response { .. } => {}
            }
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let mut payload = json!({
            "jsonrpc": "2.0",
            "method": method,
        });

        if !params.is_null() {
            payload["params"] = params;
        }

        self.socket
            .send(Message::Text(payload.to_string().into()))
            .await
            .map_err(|error| format!("failed to send app-server notification {method}: {error}"))
    }

    async fn next_message(&mut self) -> Result<IncomingMessage, String> {
        if let Some(message) = self.buffered_messages.pop_front() {
            return Ok(message);
        }

        self.read_socket_message().await
    }

    async fn read_socket_message(&mut self) -> Result<IncomingMessage, String> {
        loop {
            let message = self
                .socket
                .next()
                .await
                .ok_or_else(|| "Codex app-server connection closed unexpectedly.".to_string())?
                .map_err(|error| format!("Codex app-server connection failed: {error}"))?;

            match message {
                Message::Text(text) => {
                    let payload: Value = serde_json::from_str(&text)
                        .map_err(|error| format!("failed to parse app-server message: {error}"))?;

                    let method = payload.get("method").and_then(Value::as_str);
                    let id = payload.get("id").and_then(Value::as_u64);
                    let params = payload.get("params").cloned().unwrap_or(Value::Null);

                    if let (Some(method), Some(id)) = (method, id) {
                        return Ok(IncomingMessage::Request {
                            id,
                            method: method.to_string(),
                            params,
                        });
                    }

                    if let Some(method) = method {
                        return Ok(IncomingMessage::Notification {
                            method: method.to_string(),
                            params,
                        });
                    }

                    if let Some(id) = id {
                        return Ok(IncomingMessage::Response { id, payload });
                    }
                }
                Message::Binary(bytes) => {
                    let payload: Value = serde_json::from_slice(&bytes).map_err(|error| {
                        format!("failed to parse binary app-server message: {error}")
                    })?;
                    let id = payload.get("id").and_then(Value::as_u64).ok_or_else(|| {
                        "received unexpected binary app-server message".to_string()
                    })?;
                    return Ok(IncomingMessage::Response { id, payload });
                }
                Message::Ping(payload) => {
                    self.socket
                        .send(Message::Pong(payload))
                        .await
                        .map_err(|error| {
                            format!("failed to respond to app-server ping: {error}")
                        })?;
                }
                Message::Pong(_) => {}
                Message::Close(frame) => {
                    let note = frame
                        .map(|frame| frame.reason.to_string())
                        .filter(|reason| !reason.is_empty())
                        .unwrap_or_else(|| "without a close reason".to_string());
                    return Err(format!("Codex app-server connection closed {note}."));
                }
                _ => {}
            }
        }
    }

    async fn handle_server_request(
        &mut self,
        id: u64,
        method: &str,
        _params: Value,
    ) -> Result<(), String> {
        match method {
            "account/chatgptAuthTokens/refresh" => {
                let response = read_chatgpt_refresh_response(&self.codex_home)?;
                self.respond_result(id, response).await
            }
            _ => {
                self.respond_error(
                    id,
                    -32601,
                    format!("Unsupported app-server request: {method}"),
                )
                .await
            }
        }
    }

    async fn respond_result(&mut self, id: u64, result: Value) -> Result<(), String> {
        self.socket
            .send(Message::Text(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result,
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|error| format!("failed to respond to app-server request: {error}"))
    }

    async fn respond_error(&mut self, id: u64, code: i64, message: String) -> Result<(), String> {
        self.socket
            .send(Message::Text(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": code,
                        "message": message,
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .map_err(|error| format!("failed to send app-server error response: {error}"))
    }
}

fn error_mentions_missing_thread(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("thread") && lower.contains("not found")
}

fn should_failover_account(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("usage limit")
        || lower.contains("rate limit")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("expired")
}

fn extract_latest_assistant_reply(thread: &Value) -> Option<String> {
    let turns = thread.pointer("/thread/turns")?.as_array()?;
    let items = turns.last()?.get("items")?.as_array()?;
    let replies = items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if replies.is_empty() {
        None
    } else {
        Some(replies.join("\n\n"))
    }
}

fn read_chatgpt_refresh_response(codex_home: &Path) -> Result<Value, String> {
    let auth_path = codex_home.join("auth.json");
    let raw = fs::read_to_string(&auth_path).map_err(|error| {
        format!(
            "failed to read Codex auth file {}: {error}",
            auth_path.display()
        )
    })?;
    let auth: RuntimeAuthFile = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse Codex auth file: {error}"))?;

    let tokens = auth
        .tokens
        .ok_or_else(|| "Codex auth file had no ChatGPT tokens to refresh.".to_string())?;
    let access_token = tokens
        .access_token
        .ok_or_else(|| "Codex auth file had no ChatGPT access token.".to_string())?;
    let account_id = tokens
        .account_id
        .ok_or_else(|| "Codex auth file had no ChatGPT account id.".to_string())?;

    Ok(json!({
        "accessToken": access_token,
        "chatgptAccountId": account_id,
        "chatgptPlanType": Value::Null,
    }))
}

async fn wait_for_app_server_ready(child: &mut Child, port: u16) -> Result<(), String> {
    for _ in 0..40 {
        if let Some(exit_status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(format!(
                "Codex app-server failed to stay running: {exit_status}"
            ));
        }

        if app_server_is_ready(port).await {
            return Ok(());
        }

        sleep(Duration::from_millis(250)).await;
    }

    Err("Codex app-server did not become ready before timeout".into())
}

async fn app_server_is_ready(port: u16) -> bool {
    let ready_url = format!("http://127.0.0.1:{port}/readyz");
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    else {
        return false;
    };

    let Ok(response) = client.get(&ready_url).send().await else {
        return false;
    };

    response.status().is_success()
}

fn reserve_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    Ok(port)
}
