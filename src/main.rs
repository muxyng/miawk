mod backend;
mod theme;

use std::{sync::Arc, time::Duration};

use backend::AppServices;
use backend::accounts::{
    AccountKind, AccountSummary, AccountsPayload, DeviceAuthPoll, DeviceAuthStart,
    RateLimitSnapshot,
};
use backend::runtime::{
    ChatAttachment, ChatAttachmentKind, ChatModelInfo, ChatReasoningEffortInfo, ChatStreamEvent,
    ChatTurnSettings,
};
use backend::settings::AppSettings;
use backend::swarm::{
    SwarmAgentTreeNode, SwarmCanvasNode, SwarmNodeKind, SwarmNodeStatus, SwarmProjection,
    SwarmSnapshot,
};
use dioxus::desktop::{Config, WindowBuilder, use_window};
use dioxus::prelude::*;
use pulldown_cmark::{Options, Parser, html};
use theme::app_css;
use tracing_subscriber::EnvFilter;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Chat,
    Inbox,
    Agents,
    Canvas,
    Account,
    Models,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AutoDriveMode {
    Completion,
    OpenEnded,
}

impl AutoDriveMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completion => "completion",
            Self::OpenEnded => "open-ended",
        }
    }

    fn from_stored(value: &str) -> Self {
        match value {
            "open-ended" => Self::OpenEnded,
            _ => Self::Completion,
        }
    }
}

const DEFAULT_CONTEXT_WINDOW: u64 = 258_400;
const LONG_CONTEXT_WINDOW: u64 = 1_000_000;
const LONG_CONTEXT_COMPACT_LIMIT: u64 = 950_000;
const CONTEXT_BASELINE_TOKENS: u64 = 12_000;
const FAST_MODE_USAGE_MULTIPLIER: f64 = 1.5;
const LONG_CONTEXT_USAGE_MULTIPLIER: f64 = 2.0;
const AUTO_DRIVE_STOP_PREFIX: &str = "[[auto-drive-stop]]";
const UI_MODEL_IDS: [&str; 4] = ["gpt-5.4-pro", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"];

fn model_supports_long_context(model_id: &str) -> bool {
    matches!(model_id, "gpt-5.4-pro" | "gpt-5.4")
}

fn reset_chat_session(
    mut screen: Signal<Screen>,
    mut prompt: Signal<String>,
    mut attachments: Signal<Vec<ChatAttachment>>,
    mut messages: Signal<Vec<MessageItem>>,
    mut inbox_items: Signal<Vec<InboxItem>>,
    mut swarm: Signal<SwarmProjection>,
    mut selected_swarm_node: Signal<Option<String>>,
    mut chat_thread_id: Signal<Option<String>>,
    mut active_turn_id: Signal<Option<String>>,
    mut context_tokens: Signal<u64>,
    mut observed_context_window: Signal<Option<u64>>,
    mut auto_drive_started_at: Signal<Option<i64>>,
    mut auto_drive_completed_turns: Signal<u32>,
    mut suppress_next_auto_drive: Signal<bool>,
    mut notice: Signal<Option<String>>,
    mut codex_runtime_dialog: Signal<Option<String>>,
) {
    screen.set(Screen::Chat);
    prompt.set(String::new());
    attachments.set(Vec::new());
    messages.set(Vec::new());
    inbox_items.set(Vec::new());
    swarm.set(SwarmProjection::new());
    selected_swarm_node.set(None);
    chat_thread_id.set(None);
    active_turn_id.set(None);
    context_tokens.set(0);
    observed_context_window.set(None);
    auto_drive_started_at.set(None);
    auto_drive_completed_turns.set(0);
    suppress_next_auto_drive.set(false);
    notice.set(None);
    codex_runtime_dialog.set(None);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftMode {
    ApiKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChatMessageKind {
    User,
    Assistant,
    Reasoning,
    Activity,
    Command,
    Status,
}

#[derive(Clone, PartialEq)]
struct MessageItem {
    id: String,
    kind: ChatMessageKind,
    title: Option<String>,
    agent_label: Option<String>,
    text: String,
    details: Option<String>,
    expanded: bool,
    complete: bool,
}

#[derive(Clone, PartialEq)]
struct UsageRail {
    reset_text: String,
    percent: f64,
}

#[derive(Clone, PartialEq, Eq)]
struct InboxItem {
    id: String,
    source: String,
    text: String,
    answer_draft: String,
    resolved: bool,
}

const INBOX_QUESTION_PREFIX: &str = "[[inbox-question]]";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelAccess {
    Api,
    Both,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MockModelRow {
    name: &'static str,
    access: ModelAccess,
    intelligence: u8,
    speed: u8,
    reasoning: &'static str,
    fast: bool,
    long_context: bool,
}

fn attachment_kind_for_path(path: &str) -> ChatAttachmentKind {
    let lower = path.to_ascii_lowercase();
    if [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".avif",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
    {
        ChatAttachmentKind::Image
    } else {
        ChatAttachmentKind::File
    }
}

fn effort_label(effort: &str) -> &'static str {
    match effort {
        "none" => "None",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "XHigh",
        _ => "Medium",
    }
}

fn effort_tone_class(effort: &str) -> &'static str {
    match effort {
        "none" => "effort-title effort-title-none",
        "minimal" => "effort-title effort-title-minimal",
        "low" => "effort-title effort-title-low",
        "medium" => "effort-title effort-title-medium",
        "high" => "effort-title effort-title-high",
        "xhigh" => "effort-title effort-title-xhigh",
        _ => "effort-title effort-title-medium",
    }
}

fn reasoning_efforts(efforts: &[&str]) -> Vec<ChatReasoningEffortInfo> {
    efforts
        .iter()
        .map(|effort| ChatReasoningEffortInfo {
            effort: (*effort).to_string(),
            description: String::new(),
        })
        .collect()
}

fn fallback_model_by_id(model_id: &str) -> Option<ChatModelInfo> {
    let model = match model_id {
        "gpt-5.4-pro" => ChatModelInfo {
            id: "gpt-5.4-pro".to_string(),
            display_name: "GPT-5.4 Pro".to_string(),
            description: String::new(),
            default_effort: Some("medium".to_string()),
            supported_efforts: reasoning_efforts(&["medium", "high", "xhigh"]),
            is_default: false,
        },
        "gpt-5.4" => ChatModelInfo {
            id: "gpt-5.4".to_string(),
            display_name: "GPT-5.4".to_string(),
            description: String::new(),
            default_effort: Some("none".to_string()),
            supported_efforts: reasoning_efforts(&["none", "low", "medium", "high", "xhigh"]),
            is_default: true,
        },
        "gpt-5.4-mini" => ChatModelInfo {
            id: "gpt-5.4-mini".to_string(),
            display_name: "GPT-5.4 Mini".to_string(),
            description: String::new(),
            default_effort: Some("none".to_string()),
            supported_efforts: reasoning_efforts(&["none", "low", "medium", "high", "xhigh"]),
            is_default: false,
        },
        "gpt-5.4-nano" => ChatModelInfo {
            id: "gpt-5.4-nano".to_string(),
            display_name: "GPT-5.4 Nano".to_string(),
            description: String::new(),
            default_effort: Some("none".to_string()),
            supported_efforts: reasoning_efforts(&["none", "low", "medium", "high", "xhigh"]),
            is_default: false,
        },
        _ => return None,
    };

    Some(model)
}

fn fallback_gpt54_model() -> ChatModelInfo {
    fallback_model_by_id("gpt-5.4").unwrap_or(ChatModelInfo {
        id: "gpt-5.4".to_string(),
        display_name: "GPT-5.4".to_string(),
        description: String::new(),
        default_effort: Some("none".to_string()),
        supported_efforts: reasoning_efforts(&["none", "low", "medium", "high", "xhigh"]),
        is_default: true,
    })
}

fn preferred_effort_for_model(model: &ChatModelInfo) -> String {
    model
        .supported_efforts
        .iter()
        .find(|effort| effort.effort == "xhigh")
        .map(|effort| effort.effort.clone())
        .or_else(|| model.default_effort.clone())
        .or_else(|| {
            model
                .supported_efforts
                .first()
                .map(|effort| effort.effort.clone())
        })
        .unwrap_or_else(|| "xhigh".to_string())
}

fn display_model_label(model: &ChatModelInfo) -> String {
    match model.id.as_str() {
        "gpt-5.4-pro" => "GPT-5.4 Pro".to_string(),
        "gpt-5.4" => "GPT-5.4".to_string(),
        "gpt-5.4-mini" => "GPT-5.4 Mini".to_string(),
        "gpt-5.4-nano" => "GPT-5.4 Nano".to_string(),
        _ => model.display_name.clone(),
    }
}

fn display_model_id_label(model_id: &str) -> String {
    match model_id {
        "gpt-5.4-pro" => "GPT-5.4 Pro".to_string(),
        "gpt-5.4" => "GPT-5.4".to_string(),
        "gpt-5.4-mini" => "GPT-5.4 Mini".to_string(),
        "gpt-5.4-nano" => "GPT-5.4 Nano".to_string(),
        other => other.to_string(),
    }
}

fn display_effort_label(effort: &str) -> String {
    match effort {
        "none" => "None",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "XHigh",
        other => other,
    }
    .to_string()
}

fn supported_ui_models(models: &[ChatModelInfo]) -> Vec<ChatModelInfo> {
    UI_MODEL_IDS
        .iter()
        .filter_map(|id| {
            let live = models.iter().find(|model| model.id == *id);
            fallback_model_by_id(id).map(|mut model| {
                if let Some(live) = live {
                    if !live.description.is_empty() {
                        model.description = live.description.clone();
                    }
                    model.is_default = live.is_default || model.is_default;
                }
                model
            })
        })
        .collect()
}

fn compact_limit_for_window(context_window: u64) -> u64 {
    if context_window >= LONG_CONTEXT_WINDOW {
        LONG_CONTEXT_COMPACT_LIMIT
    } else {
        context_window
    }
}

fn display_context_window(context_window: u64) -> u64 {
    compact_limit_for_window(context_window).saturating_sub(CONTEXT_BASELINE_TOKENS)
}

fn display_context_usage(context_tokens: u64) -> u64 {
    context_tokens.saturating_sub(CONTEXT_BASELINE_TOKENS)
}

fn format_token_count(value: u64) -> String {
    if value >= 1_000_000 {
        let amount = value as f64 / 1_000_000.0;
        let text = format!("{amount:.1}");
        format!("{}M", text.trim_end_matches('0').trim_end_matches('.'))
    } else if value >= 1_000 {
        let amount = value as f64 / 1_000.0;
        let text = format!("{amount:.1}");
        format!("{}K", text.trim_end_matches('0').trim_end_matches('.'))
    } else {
        value.to_string()
    }
}

fn format_usage_multiplier(multiplier: f64) -> String {
    let text = format!("{multiplier:.1}");
    format!("{}x", text.trim_end_matches('0').trim_end_matches('.'))
}

fn context_usage_multiplier(
    fast_enabled: bool,
    raw_context_window: u64,
    raw_context_tokens: u64,
) -> f64 {
    let mut multiplier = 1.0;

    if fast_enabled {
        multiplier *= FAST_MODE_USAGE_MULTIPLIER;
    }

    let long_context_active = raw_context_window > DEFAULT_CONTEXT_WINDOW;
    if long_context_active && raw_context_tokens > DEFAULT_CONTEXT_WINDOW {
        multiplier *= LONG_CONTEXT_USAGE_MULTIPLIER;
    }

    multiplier
}

fn sanitize_settings_input(value: &str) -> String {
    value.chars().filter(|char| char.is_ascii_digit()).collect()
}

fn optional_limit_string(value: Option<u32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn parse_optional_limit_input(label: &str, value: &str) -> Result<Option<u32>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = trimmed
        .parse::<u32>()
        .map_err(|_| format!("{label} must be a whole number."))?;

    if parsed == 0 {
        return Err(format!("{label} must be greater than 0 or left blank."));
    }

    Ok(Some(parsed))
}

fn optional_limit_value(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        trimmed.parse::<u32>().ok().filter(|value| *value > 0)
    }
}

fn extract_inbox_questions(text: &str, include_trailing_partial: bool) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim();
            if !trimmed.starts_with(INBOX_QUESTION_PREFIX) {
                return None;
            }
            if !include_trailing_partial && index + 1 == lines.len() && !text.ends_with('\n') {
                return None;
            }
            let question = trimmed[INBOX_QUESTION_PREFIX.len()..].trim();
            (!question.is_empty()).then(|| question.to_string())
        })
        .collect()
}

fn extract_auto_drive_stop_reason(text: &str, include_trailing_partial: bool) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines.iter().enumerate().find_map(|(index, line)| {
        let trimmed = line.trim();
        if !trimmed.starts_with(AUTO_DRIVE_STOP_PREFIX) {
            return None;
        }
        if !include_trailing_partial && index + 1 == lines.len() && !text.ends_with('\n') {
            return None;
        }
        let reason = trimmed[AUTO_DRIVE_STOP_PREFIX.len()..].trim();
        if reason.is_empty() {
            Some("the objective appears complete".to_string())
        } else {
            Some(reason.to_string())
        }
    })
}

fn strip_assistant_protocol_lines(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with(INBOX_QUESTION_PREFIX)
                && !trimmed.starts_with(AUTO_DRIVE_STOP_PREFIX)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn message_visible_text(message: &MessageItem) -> String {
    match message.kind {
        ChatMessageKind::Assistant => strip_assistant_protocol_lines(&message.text),
        _ => message.text.clone(),
    }
}

fn source_label_for_message(message: &MessageItem) -> String {
    message
        .agent_label
        .clone()
        .or_else(|| message.title.clone())
        .unwrap_or_else(|| "Brain".to_string())
}

fn prompt_text_for_submission(user_message: &str, attachments: &[ChatAttachment]) -> String {
    if user_message.trim().is_empty() && !attachments.is_empty() {
        "Please inspect the attached files.".to_string()
    } else {
        user_message.trim().to_string()
    }
}

fn display_user_message(user_message: &str, attachments: &[ChatAttachment]) -> String {
    if attachments.is_empty() {
        user_message.trim().to_string()
    } else if user_message.trim().is_empty() {
        format!(
            "Attached: {}",
            attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "{}\n\nAttached: {}",
            user_message.trim(),
            attachments
                .iter()
                .map(|attachment| attachment.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn normalized_auto_drive_text(text: &str) -> String {
    strip_assistant_protocol_lines(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn last_completed_assistant_messages(messages: &[MessageItem], count: usize) -> Vec<MessageItem> {
    messages
        .iter()
        .rev()
        .filter(|message| message.kind == ChatMessageKind::Assistant && message.complete)
        .take(count)
        .cloned()
        .collect()
}

fn last_completed_user_message(messages: &[MessageItem]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.kind == ChatMessageKind::User && message.complete)
        .map(|message| message.text.clone())
}

fn is_creative_writing_request(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        "write me a story",
        "write a story",
        "continue the story",
        "story",
        "scene",
        "chapter",
        "poem",
        "novel",
        "short story",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn is_auto_drive_meta_response(text: &str) -> bool {
    let normalized = normalized_auto_drive_text(text);
    [
        "highest-value next step",
        "single highest-value next step",
        "best move is to",
        "continue the story rather than review it",
        "produce the next story scene now",
        "the requested story is complete",
        "if you'd like, i can also write",
        "if you want, i can also write",
        "the story is complete",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn auto_drive_frontier_excerpt(text: &str) -> String {
    let clean = strip_assistant_protocol_lines(text);
    let mut paragraphs = clean
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();

    if paragraphs.len() >= 2 {
        let tail = paragraphs.split_off(paragraphs.len() - 2);
        return tail.join("\n\n");
    }

    if clean.len() > 700 {
        clean[clean.len() - 700..].trim().to_string()
    } else {
        clean.trim().to_string()
    }
}

fn appears_to_replay_existing_text(last_text: &str, snapshot: &[MessageItem]) -> bool {
    let normalized = normalized_auto_drive_text(last_text);
    if normalized.len() < 180 {
        return false;
    }

    let prefix = &normalized[..180];
    let mut assistant_history = snapshot
        .iter()
        .filter(|message| message.kind == ChatMessageKind::Assistant && message.complete)
        .map(|message| normalized_auto_drive_text(&message.text))
        .collect::<Vec<_>>();

    let _ = assistant_history.pop();
    assistant_history
        .into_iter()
        .any(|prior| !prior.is_empty() && prior.contains(prefix))
}

fn auto_drive_prompt(
    last_assistant_text: &str,
    open_questions: usize,
    stalled: bool,
    completion_signal: bool,
    meta_loop: bool,
    replay_loop: bool,
    creative_writing: bool,
    mode: AutoDriveMode,
    completed_turns: u32,
) -> String {
    let excerpt = strip_assistant_protocol_lines(last_assistant_text);
    let excerpt = excerpt.trim();
    let excerpt = if excerpt.len() > 900 {
        format!("{}...", &excerpt[..900])
    } else {
        excerpt.to_string()
    };

    let frontier = auto_drive_frontier_excerpt(last_assistant_text);

    let steering = if mode == AutoDriveMode::OpenEnded && replay_loop && creative_writing {
        format!(
            "You are replaying earlier story text. Stop rewriting from the top. Continue only from the latest frontier below. Begin with the very next sentence after it. Do not repeat any sentence or paragraph from the excerpt. Output only new story prose.\n\nLatest frontier:\n{frontier}"
        )
    } else if mode == AutoDriveMode::OpenEnded && meta_loop && creative_writing {
        format!(
            "You are stuck in meta-commentary. Stop describing the next step and write the story itself. Continue directly in prose from the latest frontier below. No analysis, no options, no explanation, no review. Output only the next scene or passage.\n\nLatest frontier:\n{frontier}"
        )
    } else if mode == AutoDriveMode::OpenEnded && meta_loop {
        "You are stuck in meta-commentary. Stop describing the next step and perform the next step directly. Output the work itself, not a review, plan, or explanation.".to_string()
    } else if mode == AutoDriveMode::OpenEnded && creative_writing {
        format!(
            "Continue the story directly from the latest frontier below. Do not review the story, summarize what comes next, or restart from earlier sections. Output only new story text that comes after this point.\n\nLatest frontier:\n{frontier}"
        )
    } else if mode == AutoDriveMode::OpenEnded && completion_signal {
        "The obvious version of the task appears complete. Do not stop. Push beyond the first finished draft by expanding, deepening, iterating, branching into a new angle, or producing a stronger alternate pass.".to_string()
    } else if stalled {
        "You appear to be stalling or repeating yourself. Re-plan from the current state, choose a narrower next step, and avoid repeating prior summaries or actions.".to_string()
    } else {
        "Review the latest state and choose the single highest-value next step instead of repeating or summarizing old work.".to_string()
    };

    let question_policy = if open_questions > 0 {
        format!(
            "There are {open_questions} open inbox question(s). Keep going without waiting for the user unless an answer is truly required right now; if needed, state the assumption you are making and continue."
        )
    } else {
        "There are no open inbox questions right now.".to_string()
    };

    format!(
        "Auto Drive supervisor review for cycle {completed_turns}.\n\nLatest visible result:\n{excerpt}\n\n{steering}\n{question_policy}\n\n{}",
        match mode {
            AutoDriveMode::Completion => format!(
                "Treat completion as a valid stopping condition. If the objective is truly complete or you are meaningfully blocked, emit `{AUTO_DRIVE_STOP_PREFIX} <reason>` on its own line, then give a short closing note. Otherwise, keep working toward completion."
            ),
            AutoDriveMode::OpenEnded => format!(
                "Do not stop just because the first obvious version is complete. Keep going by refining, extending, or exploring another direction. Only emit `{AUTO_DRIVE_STOP_PREFIX} <reason>` if you are blocked by a hard external constraint that prevents meaningful progress. For open-ended creative tasks, continue with the next meaningful section, variation, or escalation instead of stopping early. Never answer with only meta-commentary about what you should do next; perform it."
            ),
        }
    )
}

fn export_heading_for_message(message: &MessageItem) -> String {
    match message.kind {
        ChatMessageKind::User => "You".to_string(),
        ChatMessageKind::Assistant => source_label_for_message(message),
        ChatMessageKind::Reasoning => format!("{} · Thinking", source_label_for_message(message)),
        ChatMessageKind::Activity => message
            .title
            .clone()
            .unwrap_or_else(|| source_label_for_message(message)),
        ChatMessageKind::Command => format!("{} · Command", source_label_for_message(message)),
        ChatMessageKind::Status => message
            .title
            .clone()
            .unwrap_or_else(|| "Status".to_string()),
    }
}

fn export_chat_markdown(messages: &[MessageItem], inbox_items: &[InboxItem]) -> String {
    let mut sections = vec!["# MIAWK Chat Export".to_string()];

    if messages.is_empty() {
        sections.push("_No messages in this chat yet._".to_string());
    } else {
        sections.push("## Transcript".to_string());
        for message in messages {
            if message.kind == ChatMessageKind::Status
                && !message.complete
                && message.title.is_none()
            {
                continue;
            }

            sections.push(format!("### {}", export_heading_for_message(message)));

            match message.kind {
                ChatMessageKind::Command => {
                    sections.push("```sh".to_string());
                    sections.push(message.text.clone());
                    sections.push("```".to_string());
                    if let Some(details) = &message.details {
                        if !details.trim().is_empty() {
                            sections.push("```text".to_string());
                            sections.push(details.trim_end().to_string());
                            sections.push("```".to_string());
                        }
                    }
                }
                _ => {
                    let text = message_visible_text(message);
                    if !text.trim().is_empty() {
                        sections.push(text.trim().to_string());
                    }
                }
            }
        }
    }

    let open_questions: Vec<_> = inbox_items.iter().filter(|item| !item.resolved).collect();
    if !open_questions.is_empty() {
        sections.push("## Open Questions".to_string());
        for item in open_questions {
            sections.push(format!("- **{}**: {}", item.source, item.text));
        }
    }

    sections.join("\n\n") + "\n"
}

fn parse_settings_form(
    max_threads: &str,
    max_depth: &str,
    auto_drive_mode: AutoDriveMode,
    auto_drive_runtime_hours: &str,
    auto_drive_max_turns: &str,
) -> Result<AppSettings, String> {
    let agent_max_threads = max_threads
        .trim()
        .parse::<usize>()
        .map_err(|_| "Agent threads must be a whole number.".to_string())?;
    let agent_max_depth = max_depth
        .trim()
        .parse::<i32>()
        .map_err(|_| "Agent depth must be a whole number.".to_string())?;

    if agent_max_threads < 1 {
        return Err("Agent threads must be at least 1.".to_string());
    }
    if agent_max_depth < 1 {
        return Err("Agent depth must be at least 1.".to_string());
    }

    let auto_drive_max_runtime_hours =
        parse_optional_limit_input("Auto Drive max runtime", auto_drive_runtime_hours)?;
    let auto_drive_max_turns =
        parse_optional_limit_input("Auto Drive max turns", auto_drive_max_turns)?;

    Ok(AppSettings {
        agent_max_threads,
        agent_max_depth,
        auto_drive_enabled: AppSettings::default().auto_drive_enabled,
        auto_drive_mode: auto_drive_mode.as_str().to_string(),
        auto_drive_max_turns,
        auto_drive_max_runtime_hours,
        current_workspace_path: AppSettings::default().current_workspace_path,
    })
}

fn render_markdown_html(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let parser = Parser::new_ext(text, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    ammonia::Builder::default()
        .add_tags([
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "p",
            "pre",
            "code",
            "blockquote",
            "ul",
            "ol",
            "li",
            "strong",
            "em",
            "a",
            "table",
            "thead",
            "tbody",
            "tr",
            "th",
            "td",
            "hr",
        ])
        .add_generic_attributes(["class"])
        .clean(&html_output)
        .to_string()
}

fn agent_accent_rgb(label: &str) -> &'static str {
    const COLORS: [&str; 8] = [
        "255, 146, 196",
        "255, 196, 118",
        "143, 224, 255",
        "168, 255, 188",
        "205, 166, 255",
        "255, 162, 128",
        "138, 190, 255",
        "255, 220, 126",
    ];

    let hash = label.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as u32)
    });
    COLORS[(hash as usize) % COLORS.len()]
}

fn message_agent_style(message: &MessageItem) -> Option<String> {
    message
        .agent_label
        .as_deref()
        .map(|label| format!("--agent-rgb: {}", agent_accent_rgb(label)))
}

fn mock_model_rows() -> Vec<MockModelRow> {
    let mut rows = vec![
        MockModelRow {
            name: "GPT-5.4 Pro",
            access: ModelAccess::Api,
            intelligence: 5,
            speed: 1,
            reasoning: "Medium, High, XHigh",
            fast: true,
            long_context: true,
        },
        MockModelRow {
            name: "GPT-5.4",
            access: ModelAccess::Both,
            intelligence: 4,
            speed: 3,
            reasoning: "None, Low, Medium, High, XHigh",
            fast: true,
            long_context: true,
        },
        MockModelRow {
            name: "GPT-5.4 Mini",
            access: ModelAccess::Both,
            intelligence: 3,
            speed: 4,
            reasoning: "None, Low, Medium, High, XHigh",
            fast: true,
            long_context: false,
        },
        MockModelRow {
            name: "GPT-5.4 Nano",
            access: ModelAccess::Api,
            intelligence: 2,
            speed: 5,
            reasoning: "None, Low, Medium, High, XHigh",
            fast: true,
            long_context: false,
        },
    ];

    rows.sort_by(|left, right| {
        right
            .intelligence
            .cmp(&left.intelligence)
            .then_with(|| left.name.cmp(right.name))
    });

    rows
}

fn current_chat_model(models: &[ChatModelInfo], selected_model: &str) -> Option<ChatModelInfo> {
    models
        .iter()
        .find(|model| model.id == selected_model)
        .cloned()
}

fn cycle_model(
    models: &[ChatModelInfo],
    current_model: &str,
    direction: i32,
) -> Option<ChatModelInfo> {
    if models.is_empty() {
        return None;
    }

    let current_index = models
        .iter()
        .position(|model| model.id == current_model)
        .unwrap_or(0);
    let next_index = current_index as i32 + direction;
    if next_index < 0 || next_index >= models.len() as i32 {
        return None;
    }

    models.get(next_index as usize).cloned()
}

fn cycle_effort(
    model: &ChatModelInfo,
    current_effort: &str,
    direction: i32,
) -> Option<ChatReasoningEffortInfo> {
    let options = &model.supported_efforts;
    if options.is_empty() {
        return None;
    }

    let current_index = options
        .iter()
        .position(|effort| effort.effort == current_effort)
        .unwrap_or_else(|| {
            options
                .iter()
                .position(|effort| model.default_effort.as_deref() == Some(effort.effort.as_str()))
                .unwrap_or(0)
        });

    let next_index = current_index as i32 + direction;
    if next_index < 0 || next_index >= options.len() as i32 {
        return None;
    }

    options.get(next_index as usize).cloned()
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("rsc_dioxus=info".parse().unwrap()),
        )
        .without_time()
        .init();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new().with_window(
                WindowBuilder::new()
                    .with_title("MIAWK")
                    .with_inner_size(dioxus::desktop::LogicalSize::new(1440.0, 920.0))
                    .with_min_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 720.0))
                    .with_decorations(false),
            ),
        )
        .launch(App);
}

#[component]
fn App() -> Element {
    let desktop = use_window();
    let services = use_hook(|| Arc::new(AppServices::new().expect("services to initialize")));
    use_context_provider(|| services.clone());

    let mut bootstrapped = use_signal(|| false);
    let mut screen = use_signal(|| Screen::Chat);
    let messages = use_signal(|| Vec::<MessageItem>::new());
    let mut swarm = use_signal(SwarmProjection::new);
    let mut selected_swarm_node = use_signal(|| None::<String>);
    let mut prompt = use_signal(String::new);
    let mut inbox_items = use_signal(|| Vec::<InboxItem>::new());
    let mut attachments = use_signal(|| Vec::<ChatAttachment>::new());
    let mut new_chat_armed = use_signal(|| false);
    let mut chat_models = use_signal(|| Vec::<ChatModelInfo>::new());
    let mut selected_model = use_signal(|| "gpt-5.4".to_string());
    let mut selected_effort = use_signal(|| "xhigh".to_string());
    let mut fast_mode = use_signal(|| false);
    let mut one_m_context = use_signal(|| false);
    let context_tokens = use_signal(|| 0_u64);
    let observed_context_window = use_signal(|| None::<u64>);
    let mut accounts = use_signal(|| Vec::<AccountSummary>::new());
    let mut active_account_id = use_signal(|| None::<String>);
    let mut show_account_composer = use_signal(|| false);
    let mut draft_mode = use_signal(|| None::<DraftMode>);
    let mut api_key_value = use_signal(String::new);
    let mut pending_device_auth = use_signal(|| None::<DeviceAuthStart>);
    let mut notice = use_signal(|| None::<String>);
    let codex_runtime_dialog = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let chat_busy = use_signal(|| false);
    let chat_thread_id = use_signal(|| None::<String>);
    let active_turn_id = use_signal(|| None::<String>);
    let mut settings_threads = use_signal(|| "16".to_string());
    let mut settings_depth = use_signal(|| "1".to_string());
    let mut settings_notice = use_signal(|| None::<String>);
    let mut auto_drive_enabled = use_signal(|| false);
    let mut auto_drive_mode = use_signal(|| AutoDriveMode::Completion);
    let mut auto_drive_turns = use_signal(String::new);
    let mut auto_drive_runtime = use_signal(String::new);
    let auto_drive_started_at = use_signal(|| None::<i64>);
    let auto_drive_completed_turns = use_signal(|| 0_u32);
    let mut suppress_next_auto_drive = use_signal(|| false);
    let mut countdown_started = use_signal(|| false);
    let mut now_epoch = use_signal(current_unix_timestamp);
    let stylesheet = use_hook(app_css);

    if !bootstrapped() {
        bootstrapped.set(true);
        let account_boot_services = services.clone();
        spawn(async move {
            let payload = account_boot_services.accounts.list_accounts().await;
            if let Err(error) =
                apply_payload_result(payload, accounts, active_account_id, notice, false)
            {
                tracing::warn!("initial account load failed: {error}");
            }
        });

        let model_boot_services = services.clone();
        spawn(async move {
            match model_boot_services
                .runtime
                .list_models(model_boot_services.accounts.clone())
                .await
            {
                Ok(models) => {
                    let ui_models = supported_ui_models(&models);
                    let chosen = ui_models
                        .iter()
                        .find(|model| model.id == "gpt-5.4")
                        .or_else(|| ui_models.first())
                        .or_else(|| models.iter().find(|model| model.is_default))
                        .or_else(|| models.first())
                        .cloned();
                    chat_models.set(models);
                    if let Some(model) = chosen {
                        selected_model.set(model.id.clone());
                        selected_effort.set(preferred_effort_for_model(&model));
                    }
                }
                Err(error) => {
                    tracing::warn!("model list load failed: {error}");
                }
            }
        });

        let settings_boot_services = services.clone();
        spawn(async move {
            match settings_boot_services.settings.load() {
                Ok(settings) => {
                    settings_threads.set(settings.agent_max_threads.to_string());
                    settings_depth.set(settings.agent_max_depth.to_string());
                    auto_drive_enabled.set(settings.auto_drive_enabled);
                    auto_drive_mode.set(AutoDriveMode::from_stored(&settings.auto_drive_mode));
                    auto_drive_turns.set(optional_limit_string(settings.auto_drive_max_turns));
                    auto_drive_runtime
                        .set(optional_limit_string(settings.auto_drive_max_runtime_hours));
                }
                Err(error) => {
                    tracing::warn!("settings load failed: {error}");
                    settings_notice.set(Some(error));
                }
            }
        });
    }

    if !countdown_started() {
        countdown_started.set(true);
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                now_epoch.set(current_unix_timestamp());
            }
        });
    }

    let refresh_services = services.clone();
    let remove_services = services.clone();
    let add_api_key_services = services.clone();
    let start_chatgpt_services = services.clone();
    let poll_chatgpt_services = services.clone();
    let chat_keydown_services = services.clone();
    let chat_send_services = services.clone();
    let chat_stop_services = services.clone();
    let account_refresh_effect_services = services.clone();
    let settings_refresh_effect_services = services.clone();
    let drag_desktop = desktop.clone();
    let minimize_desktop = desktop.clone();
    let maximize_desktop = desktop.clone();
    let close_desktop = desktop.clone();
    let settings_services = services.clone();
    let auto_drive_toggle_services = services.clone();
    let auto_drive_mode_services = services.clone();

    use_effect(move || {
        if screen() != Screen::Account {
            return;
        }

        let services = account_refresh_effect_services.clone();
        spawn(async move {
            if let Err(error) = apply_payload_result(
                services.accounts.refresh_all_account_limits().await,
                accounts,
                active_account_id,
                notice,
                true,
            ) {
                tracing::warn!("account limit refresh failed: {error}");
            }
        });
    });

    use_effect(move || {
        if screen() != Screen::Settings {
            return;
        }

        let services = settings_refresh_effect_services.clone();
        spawn(async move {
            match services.settings.load() {
                Ok(settings) => {
                    settings_threads.set(settings.agent_max_threads.to_string());
                    settings_depth.set(settings.agent_max_depth.to_string());
                    auto_drive_enabled.set(settings.auto_drive_enabled);
                    auto_drive_mode.set(AutoDriveMode::from_stored(&settings.auto_drive_mode));
                    auto_drive_turns.set(optional_limit_string(settings.auto_drive_max_turns));
                    auto_drive_runtime
                        .set(optional_limit_string(settings.auto_drive_max_runtime_hours));
                    settings_notice.set(None);
                }
                Err(error) => settings_notice.set(Some(error)),
            }
        });
    });

    use_effect(move || {
        let _ = prompt();
        let _ = screen();
        spawn(async move {
            let _ = document::eval(
                r#"
                const el = document.getElementById('composer-input');
                if (el) {
                  requestAnimationFrame(() => {
                    requestAnimationFrame(() => {
                      el.style.height = '0px';
                      el.style.height = `${Math.max(42, Math.min(el.scrollHeight, 180))}px`;
                    });
                  });
                }
                "#,
            )
            .await;
        });
    });

    use_effect(move || {
        if !matches!(screen(), Screen::Chat | Screen::Inbox | Screen::Agents)
            || (messages().is_empty()
                && inbox_items().is_empty()
                && prompt().trim().is_empty()
                && attachments().is_empty()
                && chat_thread_id().is_none())
        {
            new_chat_armed.set(false);
        }
    });

    use_effect(move || {
        let _ = screen();
        spawn(async move {
            let _ = document::eval(
                r#"
                const el = document.getElementById('messages-panel');
                if (!el || el.dataset.followBound === 'true') {
                  return;
                }

                const updateFollowState = () => {
                  const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
                  el.dataset.followOutput = distance <= 48 ? 'true' : 'false';
                  const jump = document.getElementById('jump-bottom-button');
                  if (jump) {
                    jump.dataset.visible = distance > 220 ? 'true' : 'false';
                  }
                };

                const handleWheel = (event) => {
                  if (event.deltaY < 0) {
                    el.dataset.followOutput = 'false';
                    return;
                  }

                  requestAnimationFrame(updateFollowState);
                };

                el.dataset.followBound = 'true';
                if (!el.dataset.followOutput) {
                  el.dataset.followOutput = 'true';
                }

                el.addEventListener('scroll', updateFollowState, { passive: true });
                el.addEventListener('wheel', handleWheel, { passive: true });
                updateFollowState();
                "#,
            )
            .await;
        });
    });

    use_effect(move || {
        let _ = messages();
        spawn(async move {
            let _ = document::eval(
                r#"
                const el = document.getElementById('messages-panel');
                if (el) {
                  const shouldFollow = el.dataset.followOutput !== 'false';
                  if (shouldFollow) {
                    el.scrollTop = el.scrollHeight;
                  }
                }
                "#,
            )
            .await;
        });
    });

    let ui_models = supported_ui_models(&chat_models());
    let current_model = current_chat_model(&ui_models, &selected_model())
        .or_else(|| ui_models.first().cloned())
        .or_else(|| Some(fallback_gpt54_model()));
    let current_model_index = ui_models.iter().position(|model| {
        model.id
            == current_model
                .as_ref()
                .map(|model| model.id.as_str())
                .unwrap_or("gpt-5.4")
    });
    let current_effort_info = current_model.as_ref().and_then(|model| {
        model
            .supported_efforts
            .iter()
            .find(|effort| effort.effort == selected_effort())
            .cloned()
            .or_else(|| model.supported_efforts.first().cloned())
    });
    let current_effort_index = current_model.as_ref().and_then(|model| {
        model
            .supported_efforts
            .iter()
            .position(|effort| effort.effort == selected_effort())
            .or_else(|| {
                model.supported_efforts.iter().position(|effort| {
                    model.default_effort.as_deref() == Some(effort.effort.as_str())
                })
            })
    });
    let can_step_effort_left = current_effort_index.is_some_and(|index| index > 0);
    let can_step_effort_right = current_model.as_ref().is_some_and(|model| {
        current_effort_index
            .map(|index| index + 1 < model.supported_efforts.len())
            .unwrap_or(false)
    });
    let can_step_model_left = current_model_index.is_some_and(|index| index > 0);
    let can_step_model_right = current_model_index
        .map(|index| index + 1 < ui_models.len())
        .unwrap_or(false);
    let current_model_label = current_model
        .as_ref()
        .map(display_model_label)
        .unwrap_or_else(|| "GPT-5.4".to_string());
    let current_model_supports_long_context = current_model
        .as_ref()
        .is_some_and(|model| model_supports_long_context(&model.id));
    let long_context_enabled = one_m_context() && current_model_supports_long_context;
    let raw_context_window = observed_context_window().unwrap_or(if long_context_enabled {
        LONG_CONTEXT_WINDOW
    } else {
        DEFAULT_CONTEXT_WINDOW
    });
    let usage_multiplier =
        context_usage_multiplier(fast_mode(), raw_context_window, context_tokens());
    let context_window = display_context_window(raw_context_window);
    let context_used = display_context_usage(context_tokens());
    let compact_progress = if context_window == 0 {
        0.0
    } else {
        (context_used as f64 / context_window as f64).clamp(0.0, 1.0)
    };
    let compact_percent = if context_window == 0 {
        0_u64
    } else {
        ((context_used as f64 / context_window as f64) * 100.0)
            .round()
            .clamp(0.0, 999.0) as u64
    };
    let visible_compact_progress = if compact_progress > 0.0 {
        compact_progress.max(0.012)
    } else {
        0.0
    };
    let mut context_label = format!(
        "{} / {} · {}%",
        format_token_count(context_used),
        format_token_count(context_window),
        compact_percent,
    );
    if usage_multiplier > 1.0 {
        context_label.push_str(" · ");
        context_label.push_str(&format_usage_multiplier(usage_multiplier));
    }
    use_effect(move || {
        let progress = visible_compact_progress;
        let _ = screen();
        let _ = prompt();
        let _ = attachments().len();
        let _ = context_tokens();
        let _ = observed_context_window();
        spawn(async move {
            let _ = document::eval(&format!(
                r#"
                const ensureMeter = (attempt = 0) => {{
                  const composer = document.getElementById('composer-shell');
                  const svg = document.getElementById('context-meter-svg');
                  const path = document.getElementById('context-meter-progress-path');
                  if (!composer || !svg || !path) {{
                    if (attempt < 10) {{
                      requestAnimationFrame(() => ensureMeter(attempt + 1));
                    }}
                    return;
                  }}

                  composer.dataset.contextProgress = '{progress:.6}';

                  if (!composer._updateContextMeter) {{
                    composer._updateContextMeter = () => {{
                      const rect = composer.getBoundingClientRect();
                      const width = Math.max(rect.width, 1);
                      const height = Math.max(rect.height, 1);
                      const inset = 0.5;
                      const radius = Math.max(0, Math.min(24, width / 2 - inset, height - inset));
                      svg.setAttribute('viewBox', `0 0 ${{width}} ${{height}}`);
                      const d = [
                        `M ${{inset}} ${{height - inset}}`,
                        `L ${{inset}} ${{radius}}`,
                        `Q ${{inset}} ${{inset}} ${{radius}} ${{inset}}`,
                        `L ${{width - radius}} ${{inset}}`,
                        `Q ${{width - inset}} ${{inset}} ${{width - inset}} ${{radius}}`,
                        `L ${{width - inset}} ${{height - inset}}`
                      ].join(' ');
                      path.setAttribute('d', d);
                      const length = path.getTotalLength();
                      const visible = length * parseFloat(composer.dataset.contextProgress || '0');
                      path.style.strokeDasharray = `${{visible}} ${{length}}`;
                      path.style.strokeDashoffset = '0';
                    }};

                    const observer = new ResizeObserver(() => {{
                      requestAnimationFrame(() => composer._updateContextMeter());
                    }});
                    observer.observe(composer);
                    composer._contextMeterObserver = observer;
                  }}

                  requestAnimationFrame(() => {{
                    requestAnimationFrame(() => composer._updateContextMeter());
                  }});
                }};

                ensureMeter();
                "#
            ))
            .await;
        });
    });
    let swarm_snapshot = swarm.read().snapshot();
    let selected_swarm_node_id = selected_swarm_node().unwrap_or_else(|| {
        swarm_snapshot
            .active_node_id
            .clone()
            .unwrap_or_else(|| swarm_snapshot.root_id.clone())
    });
    let selected_swarm_node_data = swarm_snapshot
        .nodes
        .iter()
        .find(|node| node.node.id == selected_swarm_node_id)
        .cloned();
    let open_inbox_count = inbox_items().iter().filter(|item| !item.resolved).count();
    let active_agent_count = swarm().active_agent_count();
    let active_agent_tree = swarm().active_agent_tree();
    let has_existing_chat = !messages().is_empty()
        || !inbox_items().is_empty()
        || !prompt().trim().is_empty()
        || !attachments().is_empty()
        || chat_thread_id().is_some();

    let current_model_for_sync = current_model.clone();
    use_effect(move || {
        let model = current_model_for_sync.clone();
        let current_effort = selected_effort();
        if let Some(model) = model {
            if one_m_context() && !model_supports_long_context(&model.id) {
                one_m_context.set(false);
            }
            if !model
                .supported_efforts
                .iter()
                .any(|effort| effort.effort == current_effort)
            {
                selected_effort.set(preferred_effort_for_model(&model));
            }
        }
    });

    rsx! {
        style { "{stylesheet}" }
        div {
            class: "app-shell",
            div {
                class: "frame",
                header {
                    class: "topbar",
                    nav {
                        class: "nav-cluster",
                        div {
                            class: "nav-home-anchor",
                            NavButton {
                                active: matches!(screen(), Screen::Chat | Screen::Inbox | Screen::Agents),
                                running: chat_busy(),
                                icon: "✦",
                                label: "Chat",
                                onclick: move |_| screen.set(Screen::Chat),
                            }
                            button {
                                class: if screen() == Screen::Inbox {
                                    "icon-button nav-inbox-button active"
                                } else {
                                    "icon-button nav-inbox-button"
                                },
                                aria_label: "Questions",
                                onclick: move |_| screen.set(Screen::Inbox),
                                if open_inbox_count > 0 {
                                    span {
                                        class: "nav-badge",
                                        "{open_inbox_count}"
                                    }
                                }
                                "?"
                            }
                            button {
                                class: if screen() == Screen::Agents {
                                    "icon-button nav-agents-button active"
                                } else {
                                    "icon-button nav-agents-button"
                                },
                                aria_label: "Agents",
                                onclick: move |_| screen.set(Screen::Agents),
                                if active_agent_count > 0 {
                                    span {
                                        class: "nav-badge",
                                        "{active_agent_count}"
                                    }
                                }
                                "≡"
                            }
                        }
                        NavButton {
                            active: screen() == Screen::Canvas,
                            running: false,
                            icon: "⌘",
                            label: "Canvas",
                            onclick: move |_| screen.set(Screen::Canvas),
                        }
                        NavButton {
                            active: screen() == Screen::Models,
                            running: false,
                            icon: "★",
                            label: "Models",
                            onclick: move |_| screen.set(Screen::Models),
                        }
                        NavButton {
                            active: screen() == Screen::Account,
                            running: false,
                            icon: "♫",
                            label: "Account",
                            onclick: move |_| screen.set(Screen::Account),
                        }
                    }

                    div {
                        class: "titlebar-drag",
                        onmousedown: move |_| drag_desktop.drag(),
                    }

                    div {
                        class: "window-cluster",
                        button {
                            class: if screen() == Screen::Settings {
                                "icon-button window-button active"
                            } else {
                                "icon-button window-button"
                            },
                            aria_label: "Settings",
                            onclick: move |_| screen.set(Screen::Settings),
                            "☼"
                        }
                        button {
                            class: "icon-button window-button",
                            aria_label: "Minimize",
                            onclick: move |_| minimize_desktop.set_minimized(true),
                            "-"
                        }
                        button {
                            class: "icon-button window-button",
                            aria_label: "Maximize",
                            onclick: move |_| maximize_desktop.toggle_maximized(),
                            "□"
                        }
                        button {
                            class: "icon-button window-button icon-button-destructive",
                            aria_label: "Close",
                            onclick: move |_| close_desktop.close(),
                            "X"
                        }
                    }
                }

                if matches!(screen(), Screen::Chat | Screen::Inbox | Screen::Agents) {
                    div {
                        class: "side-new-chat-anchor",
                        if has_existing_chat && new_chat_armed() {
                            div {
                                class: "new-chat-dialog side-new-chat-dialog",
                                div { class: "new-chat-dialog-label", "Start fresh?" }
                                div { class: "new-chat-dialog-copy", "Click again to clear this chat." }
                                div { class: "new-chat-dialog-tail" }
                            }
                        }
                        button {
                            class: if new_chat_armed() {
                                "icon-button side-new-chat-button new-chat-button new-chat-button-armed"
                            } else {
                                "icon-button side-new-chat-button new-chat-button"
                            },
                            aria_label: "New chat",
                            disabled: chat_busy(),
                            onclick: move |_| {
                                if chat_busy() {
                                    return;
                                }
                                if has_existing_chat && !new_chat_armed() {
                                    new_chat_armed.set(true);
                                    return;
                                }
                                reset_chat_session(
                                    screen,
                                    prompt,
                                    attachments,
                                    messages,
                                    inbox_items,
                                    swarm,
                                    selected_swarm_node,
                                    chat_thread_id,
                                    active_turn_id,
                                    context_tokens,
                                    observed_context_window,
                                    auto_drive_started_at,
                                    auto_drive_completed_turns,
                                    suppress_next_auto_drive,
                                    notice,
                                    codex_runtime_dialog,
                                );
                                new_chat_armed.set(false);
                            },
                            "►"
                        }
                    }
                    div {
                        class: "side-export-anchor",
                        button {
                            class: "icon-button side-export-button",
                            aria_label: "Export chat",
                            disabled: !has_existing_chat,
                            onclick: move |_| {
                                let export_messages = messages();
                                let export_inbox = inbox_items();
                                let mut export_notice = notice;
                                spawn(async move {
                                    let Some(file) = rfd::AsyncFileDialog::new()
                                        .set_file_name("rsc-chat-export.md")
                                        .add_filter("Markdown", &["md"])
                                        .save_file()
                                        .await
                                    else {
                                        return;
                                    };

                                    let markdown = export_chat_markdown(&export_messages, &export_inbox);
                                    match std::fs::write(file.path(), markdown) {
                                        Ok(()) => export_notice.set(Some(format!(
                                            "Exported chat to {}.",
                                            file.path().display()
                                        ))),
                                        Err(error) => export_notice
                                            .set(Some(format!("Failed to export chat: {error}"))),
                                    }
                                });
                            },
                            "⇣"
                        }
                    }
                    div {
                        class: "side-drive-anchor",
                        button {
                            class: if auto_drive_enabled() {
                                "icon-button side-drive-button active"
                            } else {
                                "icon-button side-drive-button"
                            },
                            aria_label: "Auto Drive",
                            aria_pressed: auto_drive_enabled(),
                            onclick: move |_| {
                                let mut auto_drive_started_at_signal = auto_drive_started_at;
                                let mut auto_drive_completed_turns_signal = auto_drive_completed_turns;
                                let next_enabled = !auto_drive_enabled();
                                auto_drive_enabled.set(next_enabled);
                                auto_drive_started_at_signal.set(None);
                                auto_drive_completed_turns_signal.set(0);
                                suppress_next_auto_drive.set(false);
                                let services = auto_drive_toggle_services.clone();
                                let mut toggle_notice = notice;
                                let mut toggle_auto_drive = auto_drive_enabled;
                                spawn(async move {
                                    match services.settings.load() {
                                        Ok(mut current) => {
                                            current.auto_drive_enabled = next_enabled;
                                            if let Err(error) = services.settings.save(&current) {
                                                toggle_auto_drive.set(!next_enabled);
                                                toggle_notice.set(Some(error));
                                            }
                                        }
                                        Err(error) => {
                                            toggle_auto_drive.set(!next_enabled);
                                            toggle_notice.set(Some(error));
                                        }
                                    }
                                });
                            },
                            "∞"
                        }
                    }
                }

                div {
                    class: "content",
                    if screen() == Screen::Chat {
                        div {
                            class: "chat-screen",
                            div {
                                class: "chat-main",
                                div {
                                    id: "messages-panel",
                                    class: "messages",
                                    for message in messages.read().iter() {
                                        ChatMessageRow {
                                            key: "{message.id}",
                                            message: message.clone(),
                                            on_toggle_command: move |message_id: String| {
                                                toggle_command_message(messages, &message_id);
                                            },
                                        }
                                    }
                                }
                            }

                            if let Some(message) = codex_runtime_dialog() {
                                div {
                                    class: "chat-runtime-dialog",
                                    div {
                                        class: "chat-runtime-dialog-card",
                                        div { class: "chat-runtime-dialog-title", "Codex" }
                                        div { class: "chat-runtime-dialog-copy", "{message}" }
                                    }
                                }
                            }

                            button {
                                id: "jump-bottom-button",
                                class: "jump-bottom-button",
                                aria_label: "Jump to bottom",
                                onclick: move |_| {
                                    spawn(async move {
                                        let _ = document::eval(
                                            r#"
                                            const panel = document.getElementById('messages-panel');
                                            if (panel) {
                                              panel.dataset.followOutput = 'true';
                                              panel.scrollTo({ top: panel.scrollHeight, behavior: 'smooth' });
                                            }
                                            "#,
                                        )
                                        .await;
                                    });
                                },
                                "↓"
                            }

                            div {
                                class: "composer-wrap",
                                div {
                                    id: "composer-shell",
                                    class: "composer",
                                    if !attachments().is_empty() {
                                        div {
                                            class: "composer-attachments",
                                            for attachment in attachments.read().iter() {
                                                div {
                                                    key: "attachment-{attachment.path}",
                                                    class: "attachment-chip",
                                                    span { class: "attachment-name", "{attachment.name}" }
                                                    button {
                                                        class: "attachment-remove",
                                                        aria_label: "Remove attachment",
                                                        onclick: {
                                                            let path = attachment.path.clone();
                                                            move |_| {
                                                                attachments.with_mut(|items| {
                                                                    items.retain(|item| item.path != path);
                                                                });
                                                            }
                                                        },
                                                        "×"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    textarea {
                                        id: "composer-input",
                                        rows: "1",
                                        value: prompt(),
                                        placeholder: "lay the first stone",
                                        oninput: move |event| prompt.set(event.value()),
                                        onkeydown: move |event| {
                                            if event.key() == Key::Enter && !event.modifiers().contains(Modifiers::SHIFT) {
                                                event.prevent_default();
                                                if chat_busy() {
                                                    steer_chat_prompt(
                                                        chat_keydown_services.clone(),
                                                        prompt,
                                                        messages,
                                                        chat_thread_id,
                                                        active_turn_id,
                                                        attachments,
                                                        notice,
                                                    );
                                                } else {
                                                    submit_chat_prompt(
                                                        chat_keydown_services.clone(),
                                                        prompt,
                                                        messages,
                                                        inbox_items,
                                                        swarm,
                                                        chat_busy,
                                                        chat_thread_id,
                                                        active_turn_id,
                                                        attachments,
                                                        selected_model,
                                                        selected_effort,
                                                        fast_mode,
                                                        one_m_context,
                                                        auto_drive_enabled,
                                                        auto_drive_mode,
                                                        auto_drive_started_at,
                                                        auto_drive_completed_turns,
                                                        suppress_next_auto_drive,
                                                        auto_drive_runtime,
                                                        auto_drive_turns,
                                                        context_tokens,
                                                        observed_context_window,
                                                        codex_runtime_dialog,
                                                        notice,
                                                    );
                                                }
                                            }
                                        },
                                    }
                                    div {
                                        class: "composer-row",
                                        div {
                                            class: "composer-controls",
                                            div {
                                                class: "model-cycle",
                                                button {
                                                    class: if can_step_model_left {
                                                        "model-arrow"
                                                    } else {
                                                        "model-arrow model-arrow-hidden"
                                                    },
                                                    aria_label: "Previous model",
                                                    disabled: !can_step_model_left,
                                                    onclick: {
                                                        let ui_models = ui_models.clone();
                                                        move |_| {
                                                            if let Some(next) = cycle_model(&ui_models, &selected_model(), -1) {
                                                                selected_model.set(next.id.clone());
                                                                selected_effort.set(preferred_effort_for_model(&next));
                                                            }
                                                        }
                                                    },
                                                    "‹"
                                                }
                                                div {
                                                    class: "model-current",
                                                    div {
                                                        class: "model-title",
                                                        "{current_model_label}"
                                                    }
                                                }
                                                button {
                                                    class: if can_step_model_right {
                                                        "model-arrow"
                                                    } else {
                                                        "model-arrow model-arrow-hidden"
                                                    },
                                                    aria_label: "Next model",
                                                    disabled: !can_step_model_right,
                                                    onclick: {
                                                        let ui_models = ui_models.clone();
                                                        move |_| {
                                                            if let Some(next) = cycle_model(&ui_models, &selected_model(), 1) {
                                                                selected_model.set(next.id.clone());
                                                                selected_effort.set(preferred_effort_for_model(&next));
                                                            }
                                                        }
                                                    },
                                                    "›"
                                                }
                                            }
                                            div {
                                                class: "effort-cycle",
                                                button {
                                                    class: if can_step_effort_left {
                                                        "effort-arrow"
                                                    } else {
                                                        "effort-arrow effort-arrow-hidden"
                                                    },
                                                    aria_label: "Previous reasoning effort",
                                                    disabled: !can_step_effort_left,
                                                    onclick: {
                                                        let current_model = current_model.clone();
                                                        move |_| {
                                                            if let Some(model) = &current_model {
                                                                if let Some(next) = cycle_effort(model, &selected_effort(), -1) {
                                                                    selected_effort.set(next.effort);
                                                                }
                                                            }
                                                        }
                                                    },
                                                    "‹"
                                                }
                                                div {
                                                    class: "effort-current",
                                                    div {
                                                        class: if let Some(effort) = &current_effort_info {
                                                            effort_tone_class(&effort.effort)
                                                        } else {
                                                            "effort-title effort-title-medium"
                                                        },
                                                        if let Some(effort) = &current_effort_info {
                                                            "{effort_label(&effort.effort)}"
                                                        } else {
                                                            "Effort"
                                                        }
                                                    }
                                                }
                                                button {
                                                    class: if can_step_effort_right {
                                                        "effort-arrow"
                                                    } else {
                                                        "effort-arrow effort-arrow-hidden"
                                                    },
                                                    aria_label: "Next reasoning effort",
                                                    disabled: !can_step_effort_right,
                                                    onclick: {
                                                        let current_model = current_model.clone();
                                                        move |_| {
                                                            if let Some(model) = &current_model {
                                                                if let Some(next) = cycle_effort(model, &selected_effort(), 1) {
                                                                    selected_effort.set(next.effort);
                                                                }
                                                            }
                                                        }
                                                    },
                                                    "›"
                                                }
                                            }
                                            button {
                                                class: if fast_mode() {
                                                    "mode-toggle mode-toggle-fast mode-toggle-active"
                                                } else {
                                                    "mode-toggle mode-toggle-fast"
                                                },
                                                aria_label: if fast_mode() {
                                                    "Disable fast mode"
                                                } else {
                                                    "Enable fast mode"
                                                },
                                                aria_pressed: fast_mode(),
                                                onclick: move |_| {
                                                    fast_mode.set(!fast_mode());
                                                },
                                                "Fast"
                                            }
                                            button {
                                                class: if long_context_enabled {
                                                    "mode-toggle mode-toggle-context mode-toggle-active"
                                                } else {
                                                    "mode-toggle mode-toggle-context"
                                                },
                                                disabled: !current_model_supports_long_context,
                                                aria_label: if long_context_enabled {
                                                    "Disable 1M context"
                                                } else {
                                                    "Enable 1M context"
                                                },
                                                aria_pressed: long_context_enabled,
                                                onclick: move |_| {
                                                    if !current_model_supports_long_context {
                                                        return;
                                                    }
                                                    one_m_context.set(!one_m_context());
                                                },
                                                "1M"
                                            }
                                        }
                                        div {
                                            class: "composer-actions",
                                            if chat_busy() {
                                                button {
                                                    class: "circle-button stop-button",
                                                    aria_label: "Stop",
                                                    disabled: active_turn_id().is_none() || chat_thread_id().is_none(),
                                                    onclick: move |_| {
                                                        let Some(thread_id) = chat_thread_id() else {
                                                            return;
                                                        };
                                                        let Some(turn_id) = active_turn_id() else {
                                                            return;
                                                        };
                                                        suppress_next_auto_drive.set(true);
                                                        let services = chat_stop_services.clone();
                                                        swarm.with_mut(|projection| projection.interrupt_active());
                                                        spawn(async move {
                                                            let _ = services.runtime.interrupt_turn(&thread_id, &turn_id).await;
                                                        });
                                                    },
                                                    "■"
                                                }
                                            }
                                            button {
                                                class: "circle-button",
                                                aria_label: "Attachments",
                                                onclick: move |_| {
                                                    spawn(async move {
                                                        if let Some(files) = rfd::AsyncFileDialog::new().pick_files().await {
                                                            attachments.with_mut(|items| {
                                                                for file in files {
                                                                    let path = file.path().to_string_lossy().to_string();
                                                                    if items.iter().any(|item| item.path == path) {
                                                                        continue;
                                                                    }

                                                                    items.push(ChatAttachment {
                                                                        name: file.file_name(),
                                                                        kind: attachment_kind_for_path(&path),
                                                                        path,
                                                                    });
                                                                }
                                                            });
                                                        }
                                                    });
                                                },
                                                "+"
                                            }
                                            button {
                                                class: "circle-button send-button",
                                                aria_label: "Send",
                                                disabled: prompt().trim().is_empty() && attachments().is_empty(),
                                                onclick: move |_| {
                                                    if chat_busy() {
                                                        steer_chat_prompt(
                                                            chat_send_services.clone(),
                                                            prompt,
                                                            messages,
                                                            chat_thread_id,
                                                            active_turn_id,
                                                            attachments,
                                                            notice,
                                                        );
                                                        return;
                                                    }
                                                    submit_chat_prompt(
                                                        chat_send_services.clone(),
                                                        prompt,
                                                        messages,
                                                        inbox_items,
                                                        swarm,
                                                        chat_busy,
                                                        chat_thread_id,
                                                        active_turn_id,
                                                        attachments,
                                                        selected_model,
                                                        selected_effort,
                                                        fast_mode,
                                                        one_m_context,
                                                        auto_drive_enabled,
                                                        auto_drive_mode,
                                                        auto_drive_started_at,
                                                        auto_drive_completed_turns,
                                                        suppress_next_auto_drive,
                                                        auto_drive_runtime,
                                                        auto_drive_turns,
                                                        context_tokens,
                                                        observed_context_window,
                                                        codex_runtime_dialog,
                                                        notice,
                                                    );
                                                },
                                                "↑"
                                            }
                                        }
                                    }
                                    div {
                                        class: "context-meter",
                                        svg {
                                            id: "context-meter-svg",
                                            class: "context-meter-svg",
                                            view_box: "0 0 10 10",
                                            preserve_aspect_ratio: "none",
                                            path {
                                                id: "context-meter-progress-path",
                                                class: "context-meter-progress",
                                            }
                                        }
                                        div {
                                            class: "context-meter-top-label",
                                            span {
                                                class: "context-meter-label",
                                                "{context_label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if screen() == Screen::Canvas {
                        CanvasScreen {
                            snapshot: swarm_snapshot.clone(),
                            selected_node: selected_swarm_node_data.clone(),
                            on_select_node: move |node_id| selected_swarm_node.set(Some(node_id)),
                        }
                    } else if screen() == Screen::Inbox {
                        InboxScreen {
                            items: inbox_items(),
                            busy: chat_busy(),
                            on_answer_input: move |(item_id, value)| {
                                inbox_items.with_mut(|items| {
                                    if let Some(item) = items.iter_mut().find(|item| item.id == item_id) {
                                        item.answer_draft = value;
                                    }
                                });
                            },
                            on_submit_answer: move |item_id| {
                                let selected = inbox_items()
                                    .into_iter()
                                    .find(|item| item.id == item_id && !item.resolved);
                                let Some(item) = selected else {
                                    return;
                                };
                                let answer = item.answer_draft.trim().to_string();
                                if answer.is_empty() {
                                    notice.set(Some("Write an answer before sending it.".to_string()));
                                    return;
                                }
                                inbox_items.with_mut(|items| {
                                    if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
                                        existing.resolved = true;
                                    }
                                });
                                let prompt_text = format!(
                                    "Answer to inbox question: {}\n\n{}",
                                    item.text, answer
                                );
                                let display_text = format!("Q: {}\nA: {}", item.text, answer);
                                if chat_busy() {
                                    steer_arbitrary_prompt(
                                        chat_send_services.clone(),
                                        prompt_text,
                                        display_text,
                                        messages,
                                        chat_thread_id(),
                                        active_turn_id(),
                                        notice,
                                    );
                                } else {
                                    submit_arbitrary_prompt(
                                        chat_send_services.clone(),
                                        prompt_text,
                                        display_text,
                                        true,
                                        true,
                                        messages,
                                        inbox_items,
                                        swarm,
                                        chat_busy,
                                        chat_thread_id,
                                        active_turn_id,
                                        selected_model,
                                        selected_effort,
                                        fast_mode,
                                        one_m_context,
                                        auto_drive_enabled,
                                        auto_drive_mode,
                                        auto_drive_started_at,
                                        auto_drive_completed_turns,
                                        suppress_next_auto_drive,
                                        auto_drive_runtime,
                                        auto_drive_turns,
                                        context_tokens,
                                        observed_context_window,
                                        codex_runtime_dialog,
                                        notice,
                                    );
                                }
                            },
                            on_resolve: move |item_id| {
                                inbox_items.with_mut(|items| {
                                    if let Some(item) = items.iter_mut().find(|item| item.id == item_id) {
                                        item.resolved = true;
                                    }
                                });
                            },
                        }
                    } else if screen() == Screen::Agents {
                        AgentsScreen {
                            count: active_agent_count,
                            items: active_agent_tree.clone(),
                        }
                    } else if screen() == Screen::Account {
                        AccountScreen {
                            accounts: accounts(),
                            now_epoch: now_epoch(),
                            notice: notice(),
                            show_account_composer: show_account_composer(),
                            draft_mode: draft_mode(),
                            api_key_value: api_key_value(),
                            pending_device_auth: pending_device_auth(),
                            busy: busy(),
                            on_toggle_composer: move |_| {
                                let opening = !show_account_composer();
                                show_account_composer.set(opening);
                                if opening && pending_device_auth().is_none() {
                                    draft_mode.set(None);
                                }
                            },
                            on_set_draft_mode: move |mode| draft_mode.set(mode),
                            on_api_key_value: move |value| api_key_value.set(value),
                            on_refresh: move |_| {
                                let services = refresh_services.clone();
                                busy.set(true);
                                spawn(async move {
                                    let payload = services.accounts.refresh_all_account_limits().await;
                                    let _ = apply_payload_result(payload, accounts, active_account_id, notice, true);
                                    busy.set(false);
                                });
                            },
                            on_remove: move |account_id| {
                                let services = remove_services.clone();
                                busy.set(true);
                                spawn(async move {
                                    let payload = services.accounts.remove_account(account_id).await;
                                    let _ = apply_payload_result(payload, accounts, active_account_id, notice, false);
                                    busy.set(false);
                                });
                            },
                            on_add_api_key: move |_| {
                                let services = add_api_key_services.clone();
                                let secret = api_key_value();
                                if secret.trim().is_empty() {
                                    notice.set(Some("Paste an API key before linking it.".to_string()));
                                    return;
                                }
                                busy.set(true);
                                spawn(async move {
                                    let payload = services.accounts.add_api_key_account(secret).await;
                                    let applied = apply_payload_result(payload, accounts, active_account_id, notice, true);
                                    if applied.is_ok() {
                                        api_key_value.set(String::new());
                                        draft_mode.set(None);
                                        show_account_composer.set(false);
                                    }
                                    busy.set(false);
                                });
                            },
                            on_start_chatgpt: move |_| {
                                let services = start_chatgpt_services.clone();
                                busy.set(true);
                                spawn(async move {
                                    match services.accounts.start_chatgpt_account_link().await {
                                        Ok(start) => {
                                            let _ = webbrowser::open(&start.verification_uri);
                                            pending_device_auth.set(Some(start));
                                            notice.set(Some("Complete the device-auth flow in your browser, then press Check link.".to_string()));
                                        }
                                        Err(error) => notice.set(Some(error)),
                                    }
                                    busy.set(false);
                                });
                            },
                            on_poll_chatgpt: move |_| {
                                let Some(start) = pending_device_auth() else {
                                    return;
                                };
                                let services = poll_chatgpt_services.clone();
                                busy.set(true);
                                spawn(async move {
                                    match services.accounts.poll_chatgpt_account_link(start.pending_id.clone()).await {
                                        Ok(DeviceAuthPoll::Pending { verification_uri, user_code, .. }) => {
                                            pending_device_auth.set(Some(DeviceAuthStart { pending_id: start.pending_id.clone(), verification_uri, user_code }));
                                            notice.set(Some("Still waiting for the browser flow to finish.".to_string()));
                                        }
                                        Ok(DeviceAuthPoll::Complete { payload, .. }) => {
                                            accounts.set(payload.accounts);
                                            active_account_id.set(payload.active_account_id);
                                            pending_device_auth.set(None);
                                            draft_mode.set(None);
                                            show_account_composer.set(false);
                                            notice.set(Some("ChatGPT account linked.".to_string()));
                                        }
                                        Err(error) => notice.set(Some(error)),
                                    }
                                    busy.set(false);
                                });
                            },
                        }
                    } else if screen() == Screen::Settings {
                        SettingsScreen {
                            threads_value: settings_threads(),
                            depth_value: settings_depth(),
                            auto_drive_enabled: auto_drive_enabled(),
                            auto_drive_mode: auto_drive_mode(),
                            auto_drive_runtime_hours_value: auto_drive_runtime(),
                            auto_drive_max_turns_value: auto_drive_turns(),
                            notice: settings_notice(),
                            busy: busy(),
                            on_threads_input: move |value| {
                                let value: String = value;
                                settings_threads.set(sanitize_settings_input(&value));
                            },
                            on_depth_input: move |value| {
                                let value: String = value;
                                settings_depth.set(sanitize_settings_input(&value));
                            },
                            on_auto_drive_runtime_input: move |value| {
                                let value: String = value;
                                auto_drive_runtime.set(sanitize_settings_input(&value));
                            },
                            on_set_auto_drive_mode: move |mode| {
                                auto_drive_mode.set(mode);
                                let services = auto_drive_mode_services.clone();
                                let mut mode_notice = settings_notice;
                                spawn(async move {
                                    match services.settings.load() {
                                        Ok(mut current) => {
                                            current.auto_drive_mode = mode.as_str().to_string();
                                            if let Err(error) = services.settings.save(&current) {
                                                mode_notice.set(Some(error));
                                            } else {
                                                mode_notice.set(Some(format!(
                                                    "Auto Drive mode set to {}.",
                                                    mode.as_str()
                                                )));
                                            }
                                        }
                                        Err(error) => mode_notice.set(Some(error)),
                                    }
                                });
                            },
                            on_auto_drive_turns_input: move |value| {
                                let value: String = value;
                                auto_drive_turns.set(sanitize_settings_input(&value));
                            },
                            on_save: move |_| {
                                let parsed = parse_settings_form(
                                    &settings_threads(),
                                    &settings_depth(),
                                    auto_drive_mode(),
                                    &auto_drive_runtime(),
                                    &auto_drive_turns(),
                                );
                                let Ok(parsed_settings) = parsed else {
                                    settings_notice.set(parsed.err());
                                    return;
                                };
                                let services = settings_services.clone();
                                busy.set(true);
                                settings_notice.set(None);
                                spawn(async move {
                                    let mut app_settings = services
                                        .settings
                                        .load()
                                        .unwrap_or_else(|_| AppSettings::default());
                                    app_settings.agent_max_threads = parsed_settings.agent_max_threads;
                                    app_settings.agent_max_depth = parsed_settings.agent_max_depth;
                                    app_settings.auto_drive_mode = parsed_settings.auto_drive_mode;
                                    app_settings.auto_drive_max_runtime_hours =
                                        parsed_settings.auto_drive_max_runtime_hours;
                                    app_settings.auto_drive_max_turns =
                                        parsed_settings.auto_drive_max_turns;
                                    let result = services.settings.save(&app_settings);
                                    match result {
                                        Ok(()) => {
                                            if let Err(error) = services.runtime.restart().await {
                                                settings_notice.set(Some(error));
                                            } else {
                                                settings_notice.set(Some("Settings saved. New swarm runs will use the updated Codex limits.".to_string()));
                                            }
                                        }
                                        Err(error) => settings_notice.set(Some(error)),
                                    }
                                    busy.set(false);
                                });
                            },
                        }
                    } else {
                        ModelsScreen {
                            rows: mock_model_rows(),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NavButton(
    active: bool,
    running: bool,
    icon: &'static str,
    label: &'static str,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: if active && running {
                "icon-button active nav-running"
            } else if active {
                "icon-button active"
            } else if running {
                "icon-button nav-running"
            } else {
                "icon-button"
            },
            aria_label: label,
            onclick: move |event| onclick.call(event),
            "{icon}"
        }
    }
}

#[component]
fn ChatMessageRow(message: MessageItem, on_toggle_command: EventHandler<String>) -> Element {
    let message_id = message.id.clone();
    let has_details = message
        .details
        .as_ref()
        .is_some_and(|details| !details.is_empty());
    let visible_text = message_visible_text(&message);

    rsx! {
        div {
            id: "chat-message-{message.id}",
            class: chat_message_class(&message),
            style: message_agent_style(&message).unwrap_or_default(),
            if let Some(title) = &message.title {
                div { class: "message-title", "{title}" }
            }

            if message.kind == ChatMessageKind::Command {
                button {
                    class: if has_details {
                        "command-toggle command-toggle-expandable"
                    } else {
                        "command-toggle"
                    },
                    disabled: !has_details,
                    onclick: move |_| on_toggle_command.call(message_id.clone()),
                    span { class: "command-text", "{message.text}" }
                    if has_details {
                        span {
                            class: if message.expanded {
                                "command-caret expanded"
                            } else {
                                "command-caret"
                            },
                            "▾"
                        }
                    }
                }

                if message.expanded {
                    if let Some(details) = &message.details {
                        if !details.is_empty() {
                            div { class: "command-output", "{details}" }
                        }
                    }
                }
            } else if message.kind == ChatMessageKind::Status && !message.complete && message.title.is_none() {
                div {
                    class: "message-loader",
                    aria_label: "Loading",
                    span { class: "message-loader-dot", "." }
                    span { class: "message-loader-dot", "." }
                    span { class: "message-loader-dot", "." }
                }
            } else if !visible_text.is_empty() {
                MarkdownBlock {
                    class: "message-text",
                    content: visible_text,
                }
            }
        }
    }
}

#[component]
fn MarkdownBlock(class: &'static str, content: String) -> Element {
    let html = render_markdown_html(&content);
    rsx! {
        div {
            class: class,
            dangerous_inner_html: "{html}"
        }
    }
}

fn upsert_inbox_items_from_message(
    mut inbox_items: Signal<Vec<InboxItem>>,
    message: &MessageItem,
    include_trailing_partial: bool,
) {
    if message.kind != ChatMessageKind::Assistant {
        return;
    }

    let questions = extract_inbox_questions(&message.text, include_trailing_partial);
    if questions.is_empty() {
        return;
    }

    let source = source_label_for_message(message);
    inbox_items.with_mut(|items| {
        for (index, question) in questions.into_iter().enumerate() {
            let inbox_id = format!("{}:{index}", message.id);
            if let Some(existing) = items.iter_mut().find(|item| item.id == inbox_id) {
                existing.text = question.clone();
                existing.source = source.clone();
            } else {
                items.push(InboxItem {
                    id: inbox_id,
                    source: source.clone(),
                    text: question,
                    answer_draft: String::new(),
                    resolved: false,
                });
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
#[component]
fn AccountScreen(
    accounts: Vec<AccountSummary>,
    now_epoch: i64,
    notice: Option<String>,
    show_account_composer: bool,
    draft_mode: Option<DraftMode>,
    api_key_value: String,
    pending_device_auth: Option<DeviceAuthStart>,
    busy: bool,
    on_toggle_composer: EventHandler<MouseEvent>,
    on_set_draft_mode: EventHandler<Option<DraftMode>>,
    on_api_key_value: EventHandler<String>,
    on_refresh: EventHandler<MouseEvent>,
    on_remove: EventHandler<String>,
    on_add_api_key: EventHandler<MouseEvent>,
    on_start_chatgpt: EventHandler<MouseEvent>,
    on_poll_chatgpt: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        section {
            class: "accounts-screen",
            div {
                class: "accounts-header",
                div {
                    class: "accounts-count",
                    "{accounts.len()}"
                }
                div {
                    class: "accounts-actions",
                    button {
                        class: "icon-button",
                        aria_label: "Refresh account usage",
                        onclick: move |event| on_refresh.call(event),
                        "◌"
                    }
                    button {
                        class: "icon-button",
                        aria_label: "Add account or API key",
                        onclick: move |event| on_toggle_composer.call(event),
                        "+"
                    }
                }
            }

            if show_account_composer {
                section {
                    class: "modal",
                    if pending_device_auth.is_none() && draft_mode.is_none() {
                        div {
                            class: "toggle-row",
                            button {
                                class: "active",
                                onclick: move |_| on_set_draft_mode.call(Some(DraftMode::ApiKey)),
                                "API Key"
                            }
                            button {
                                onclick: move |event| on_start_chatgpt.call(event),
                                if busy { "Starting..." } else { "Account" }
                            }
                        }
                    }

                    if draft_mode == Some(DraftMode::ApiKey) {
                        div {
                            class: "input-row",
                            div {
                                class: "field",
                                label { "API key" }
                                input {
                                    value: api_key_value,
                                    placeholder: "sk-...",
                                    oninput: move |event| on_api_key_value.call(event.value()),
                                }
                            }
                        }
                        div {
                            class: "modal-actions",
                            button {
                                class: "text-button",
                                onclick: move |_| on_set_draft_mode.call(None),
                                "Back"
                            }
                            button {
                                onclick: move |event| on_add_api_key.call(event),
                                if busy { "Linking..." } else { "Link API key" }
                            }
                        }
                    }

                    if let Some(start) = pending_device_auth {
                        div {
                            class: "pending-box",
                            div {
                                class: "muted",
                                "Code: {start.user_code}"
                            }
                            div {
                                class: "modal-actions",
                                button {
                                    onclick: move |event| on_poll_chatgpt.call(event),
                                    if busy { "Checking..." } else { "Check link" }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(text) = notice {
                p {
                    class: "notice",
                    "{text}"
                }
            }

            div {
                class: "accounts-list",
                if accounts.is_empty() {
                    div {
                        class: "message",
                        "No linked accounts yet. Use the + button to link an API key or start the ChatGPT browser flow."
                    }
                } else {
                    {accounts.into_iter().map(|account| {
                        let remove_id = account.id.clone();
                        rsx! {
                            AccountEntry {
                                now_epoch,
                                on_remove: move |_| on_remove.call(remove_id.clone()),
                                account,
                            }
                        }
                    })}
                }
            }
        }
    }
}

#[component]
fn AccountEntry(
    account: AccountSummary,
    now_epoch: i64,
    on_remove: EventHandler<MouseEvent>,
) -> Element {
    let rails = usage_rails(&account.rate_limits, &account.kind, now_epoch);
    let has_rails = !rails.is_empty();
    let title = account_title(&account);
    let subtitle = account_subtitle(&account);

    rsx! {
        div {
            class: if has_rails {
                "entry"
            } else {
                "entry no-rails"
            },
            div {
                class: "entry-dot",
                aria_label: "Account available",
            }
            div {
                class: "entry-copy",
                div { class: "entry-title", "{title}" }
                div { class: "entry-subtitle", "{subtitle}" }
            }
            if has_rails {
                div { class: "entry-rails",
                    for rail in rails {
                        div {
                            class: "rail",
                            div {
                                class: "rail-track",
                                span { class: "rail-reset", "{rail.reset_text}" }
                                div {
                                    class: "rail-bar",
                                    div {
                                        class: "rail-fill",
                                        style: format!("width: {}%;", rail.percent),
                                    }
                                }
                            }
                            span { class: "rail-percent", "{rail.percent.round()}%" }
                        }
                    }
                }
            }
            div {
                class: "entry-actions",
                button {
                    class: "icon-button icon-button-destructive",
                    aria_label: "Remove account",
                    onclick: move |event| on_remove.call(event),
                    "X"
                }
            }
        }
    }
}

#[component]
fn ModelsScreen(rows: Vec<MockModelRow>) -> Element {
    rsx! {
        section {
            class: "models-screen",
            div {
                class: "models-header",
                div {
                    class: "models-count",
                    "{rows.len()}"
                }
            }

            div {
                class: "models-table",
                div {
                    class: "models-row models-row-head",
                    div { class: "models-cell models-head-cell models-model", "Model" }
                    div { class: "models-cell models-head-cell models-intelligence", "Intelligence" }
                    div { class: "models-cell models-head-cell models-speed", "Speed" }
                    div { class: "models-cell models-head-cell models-access", "Access" }
                    div { class: "models-cell models-head-cell models-reasoning", "Reasoning" }
                    div { class: "models-cell models-head-cell models-fast", "Fast" }
                    div { class: "models-cell models-head-cell models-context", "1M" }
                }
                for row in rows {
                    div {
                        class: "models-row",
                        div { class: "models-cell models-model", "{row.name}" }
                        div {
                            class: "models-cell models-intelligence",
                            div {
                                class: "star-stack star-stack-intelligence",
                                for index in 0..row.intelligence {
                                    span {
                                        key: "{row.name}-star-{index}",
                                        class: "star-mark star-mark-intelligence",
                                        style: format!("transform: translateX({}px); z-index: {};", i32::from(index) * 8, 10 - i32::from(index)),
                                        "★"
                                    }
                                }
                            }
                        }
                        div {
                            class: "models-cell models-speed",
                            div {
                                class: "star-stack star-stack-speed",
                                for index in 0..row.speed {
                                    span {
                                        key: "{row.name}-speed-star-{index}",
                                        class: "star-mark star-mark-speed",
                                        style: format!("transform: translateX({}px); z-index: {};", i32::from(index) * 8, 10 - i32::from(index)),
                                        "★"
                                    }
                                }
                            }
                        }
                        div {
                            class: "models-cell models-access",
                            span {
                                class: match row.access {
                                    ModelAccess::Api => "access-chip access-chip-api",
                                    ModelAccess::Both => "access-chip access-chip-both",
                                },
                                span { class: "access-mark", "♦" }
                                match row.access {
                                    ModelAccess::Api => "API",
                                    ModelAccess::Both => "Both",
                                }
                            }
                        }
                        div { class: "models-cell models-reasoning", "{row.reasoning}" }
                        div {
                            class: "models-cell models-fast",
                            span {
                                class: if row.fast { "trait-icon trait-icon-yes" } else { "trait-icon trait-icon-no" },
                                if row.fast { "√" } else { "X" }
                            }
                        }
                        div {
                            class: "models-cell models-context",
                            span {
                                class: if row.long_context { "trait-icon trait-icon-yes" } else { "trait-icon trait-icon-no" },
                                if row.long_context { "√" } else { "X" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SettingsScreen(
    threads_value: String,
    depth_value: String,
    auto_drive_enabled: bool,
    auto_drive_mode: AutoDriveMode,
    auto_drive_runtime_hours_value: String,
    auto_drive_max_turns_value: String,
    notice: Option<String>,
    busy: bool,
    on_threads_input: EventHandler<String>,
    on_depth_input: EventHandler<String>,
    on_auto_drive_runtime_input: EventHandler<String>,
    on_set_auto_drive_mode: EventHandler<AutoDriveMode>,
    on_auto_drive_turns_input: EventHandler<String>,
    on_save: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        section {
            class: "settings-screen",
            div {
                class: "settings-page-header",
                div {
                    class: "settings-page-count",
                    "Settings"
                }
            }

            section {
                class: "settings-panel",
                div {
                    class: "input-row",
                    div {
                        class: "field",
                        label { "Agent threads" }
                        input {
                            value: threads_value,
                            inputmode: "numeric",
                            oninput: move |event| on_threads_input.call(event.value()),
                        }
                    }
                    div {
                        class: "field",
                        label { "Agent depth" }
                        input {
                            value: depth_value,
                            inputmode: "numeric",
                            oninput: move |event| on_depth_input.call(event.value()),
                        }
                    }
                }
                div {
                    class: "settings-divider-label",
                    "Auto Drive"
                }
                p {
                    class: "muted settings-inline-note",
                    if auto_drive_enabled {
                        "Auto Drive is currently enabled from the chat rail. Mode and limits shape how it continues. Leave either field blank for no cap."
                    } else {
                        "Auto Drive is currently disabled. Mode and limits are ready for when you turn it on from the chat rail. Leave either field blank for no cap."
                    }
                }
                div {
                    class: "settings-mode-row",
                    button {
                        class: if auto_drive_mode == AutoDriveMode::Completion {
                            "mode-toggle mode-toggle-active settings-mode-toggle"
                        } else {
                            "mode-toggle settings-mode-toggle"
                        },
                        aria_pressed: auto_drive_mode == AutoDriveMode::Completion,
                        onclick: move |_| on_set_auto_drive_mode.call(AutoDriveMode::Completion),
                        "Completion"
                    }
                    button {
                        class: if auto_drive_mode == AutoDriveMode::OpenEnded {
                            "mode-toggle mode-toggle-active settings-mode-toggle"
                        } else {
                            "mode-toggle settings-mode-toggle"
                        },
                        aria_pressed: auto_drive_mode == AutoDriveMode::OpenEnded,
                        onclick: move |_| on_set_auto_drive_mode.call(AutoDriveMode::OpenEnded),
                        "Open-ended"
                    }
                }
                div {
                    class: "input-row",
                    div {
                        class: "field",
                        label { "Max runtime hours" }
                        input {
                            value: auto_drive_runtime_hours_value,
                            placeholder: "Unlimited",
                            inputmode: "numeric",
                            oninput: move |event| on_auto_drive_runtime_input.call(event.value()),
                        }
                    }
                    div {
                        class: "field",
                        label { "Max turns" }
                        input {
                            value: auto_drive_max_turns_value,
                            placeholder: "Unlimited",
                            inputmode: "numeric",
                            oninput: move |event| on_auto_drive_turns_input.call(event.value()),
                        }
                    }
                }
                p {
                    class: "muted",
                    "Swarm limits apply to new Codex swarm runs. Saving restarts the managed Codex app-server so the next run picks them up. Auto Drive limits are saved alongside them."
                }
                if let Some(text) = notice {
                    p {
                        class: "notice settings-notice",
                        "{text}"
                    }
                }
                div {
                    class: "modal-actions settings-actions",
                    button {
                        disabled: busy,
                        onclick: move |event| on_save.call(event),
                        if busy { "Saving..." } else { "Save" }
                    }
                }
            }
        }
    }
}

#[component]
fn InboxScreen(
    items: Vec<InboxItem>,
    busy: bool,
    on_answer_input: EventHandler<(String, String)>,
    on_submit_answer: EventHandler<String>,
    on_resolve: EventHandler<String>,
) -> Element {
    let open_items = items
        .iter()
        .filter(|item| !item.resolved)
        .cloned()
        .collect::<Vec<_>>();

    rsx! {
        section {
            class: "inbox-screen",
            div {
                class: "inbox-header",
                div {
                    class: "inbox-count",
                    "{open_items.len()}"
                }
            }

            div {
                class: "inbox-list",
                if open_items.is_empty() {
                    div {
                        class: "inbox-empty",
                        "No open questions."
                    }
                } else {
                    for item in open_items {
                        article {
                            key: "{item.id}",
                            class: "inbox-card",
                            div {
                                class: "inbox-card-top",
                                div { class: "inbox-source", "{item.source}" }
                                button {
                                    class: "icon-button icon-button-destructive inbox-dismiss",
                                    aria_label: "Dismiss question",
                                    onclick: {
                                        let id = item.id.clone();
                                        move |_| on_resolve.call(id.clone())
                                    },
                                    "X"
                                }
                            }
                            div { class: "inbox-question", "{item.text}" }
                            textarea {
                                class: "inbox-answer-input",
                                rows: "2",
                                value: item.answer_draft.clone(),
                                placeholder: "Answer",
                                oninput: {
                                    let id = item.id.clone();
                                    move |event| on_answer_input.call((id.clone(), event.value()))
                                },
                            }
                            div {
                                class: "inbox-actions",
                                button {
                                    disabled: busy || item.answer_draft.trim().is_empty(),
                                    onclick: {
                                        let id = item.id.clone();
                                        move |_| on_submit_answer.call(id.clone())
                                    },
                                    if busy { "Sending..." } else { "Send answer" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AgentsScreen(count: usize, items: Vec<SwarmAgentTreeNode>) -> Element {
    rsx! {
        section {
            class: "agents-screen",
            div {
                class: "agents-header",
                div {
                    class: "agents-count",
                    "{count}"
                }
            }

            div {
                class: "agents-list",
                if items.is_empty() {
                    div {
                        class: "agents-empty",
                        "No active agents."
                    }
                } else {
                    ul {
                        class: "agent-tree-root",
                        for item in items {
                            AgentTreeItem { node: item }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AgentTreeItem(node: SwarmAgentTreeNode) -> Element {
    let status_class = match node.status {
        SwarmNodeStatus::Queued => "agent-tree-node-queued",
        SwarmNodeStatus::Running => "agent-tree-node-running",
        SwarmNodeStatus::Waiting => "agent-tree-node-waiting",
        SwarmNodeStatus::Complete => "agent-tree-node-complete",
        SwarmNodeStatus::Failed => "agent-tree-node-failed",
        SwarmNodeStatus::Interrupted => "agent-tree-node-interrupted",
        SwarmNodeStatus::Idle => "agent-tree-node-idle",
    };
    let mut meta = Vec::new();
    if let Some(model) = node.model.as_deref() {
        meta.push(display_model_id_label(model));
    }
    if let Some(reasoning) = node.reasoning.as_deref() {
        meta.push(display_effort_label(reasoning));
    }
    let meta_text = meta.join(" | ");

    rsx! {
        li {
            class: "agent-tree-item",
            div {
                class: "agent-tree-row {status_class}",
                div { class: "agent-tree-title", "{node.title}" }
                div { class: "agent-tree-status", "{node.subtitle}" }
                if !meta.is_empty() {
                    div {
                        class: "agent-tree-meta",
                        "{meta_text}"
                    }
                }
                if !node.detail.trim().is_empty() {
                    div { class: "agent-tree-detail", "{node.detail}" }
                }
            }
            if !node.children.is_empty() {
                ul {
                    class: "agent-tree-children",
                    for child in node.children {
                        AgentTreeItem { node: child }
                    }
                }
            }
        }
    }
}

#[component]
fn CanvasScreen(
    snapshot: SwarmSnapshot,
    selected_node: Option<SwarmCanvasNode>,
    on_select_node: EventHandler<String>,
) -> Element {
    let node_count = snapshot.total_nodes;
    let root_node = snapshot
        .nodes
        .iter()
        .find(|node| node.node.id == snapshot.root_id)
        .cloned();
    use_effect(move || {
        let _ = node_count;
        spawn(async move {
            let _ = document::eval(
                r#"
                const viewport = document.getElementById('swarm-canvas-viewport');
                const stage = document.getElementById('swarm-canvas-stage');
                if (!viewport || !stage) {
                  return;
                }

                if (!viewport._swarmState) {
                  const rootX = Number(viewport.dataset.rootX || '0');
                  const rootY = Number(viewport.dataset.rootY || '0');
                  const rootWidth = Number(viewport.dataset.rootWidth || '220');
                  const rootHeight = Number(viewport.dataset.rootHeight || '112');
                  const scale = 0.9;
                  viewport._swarmState = {
                    panX: viewport.clientWidth * 0.5 - (rootX + rootWidth * 0.5) * scale,
                    panY: viewport.clientHeight * 0.5 - (rootY + rootHeight * 0.5) * scale,
                    scale,
                    dragging: false,
                    startX: 0,
                    startY: 0,
                  };
                }

                const state = viewport._swarmState;
                const apply = () => {
                  stage.style.transform = `translate(${state.panX}px, ${state.panY}px) scale(${state.scale})`;
                };

                if (viewport.dataset.bound !== 'true') {
                  viewport.dataset.bound = 'true';
                  viewport.addEventListener('wheel', (event) => {
                    event.preventDefault();
                    const direction = event.deltaY < 0 ? 1.08 : 0.92;
                    state.scale = Math.min(1.8, Math.max(0.45, state.scale * direction));
                    apply();
                  }, { passive: false });

                  viewport.addEventListener('pointerdown', (event) => {
                    if (event.target.closest('.swarm-node')) {
                      return;
                    }
                    state.dragging = true;
                    state.startX = event.clientX - state.panX;
                    state.startY = event.clientY - state.panY;
                    viewport.setPointerCapture(event.pointerId);
                    viewport.dataset.dragging = 'true';
                  });

                  viewport.addEventListener('pointermove', (event) => {
                    if (!state.dragging) {
                      return;
                    }
                    state.panX = event.clientX - state.startX;
                    state.panY = event.clientY - state.startY;
                    apply();
                  });

                  const stopDrag = (event) => {
                    if (!state.dragging) {
                      return;
                    }
                    state.dragging = false;
                    viewport.dataset.dragging = 'false';
                    if (viewport.hasPointerCapture(event.pointerId)) {
                      viewport.releasePointerCapture(event.pointerId);
                    }
                  };

                  viewport.addEventListener('pointerup', stopDrag);
                  viewport.addEventListener('pointercancel', stopDrag);
                }

                apply();
                "#,
            )
            .await;
        });
    });

    let node_lookup: std::collections::HashMap<String, SwarmCanvasNode> = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| (node.node.id.clone(), node))
        .collect();
    let selected_id = selected_node.as_ref().map(|node| node.node.id.clone());

    rsx! {
        section {
            class: "canvas-screen",
            div {
            class: "canvas-header",
            div { class: "canvas-count", "{snapshot.total_nodes}" }
        }

                div {
                    class: "canvas-shell",
                div {
                    id: "swarm-canvas-viewport",
                    class: "swarm-canvas-viewport",
                    "data-root-x": root_node.as_ref().map(|node| node.x.to_string()).unwrap_or_else(|| "0".to_string()),
                    "data-root-y": root_node.as_ref().map(|node| node.y.to_string()).unwrap_or_else(|| "0".to_string()),
                    "data-root-width": root_node.as_ref().map(|node| node.width.to_string()).unwrap_or_else(|| "220".to_string()),
                    "data-root-height": root_node.as_ref().map(|node| node.height.to_string()).unwrap_or_else(|| "112".to_string()),
                    div { class: "swarm-canvas-grid" }
                    div {
                        id: "swarm-canvas-stage",
                        class: "swarm-canvas-stage",
                        for edge in snapshot.edges.iter() {
                            if let (Some(from), Some(to)) = (
                                node_lookup.get(&edge.from_id),
                                node_lookup.get(&edge.to_id),
                            ) {
                                div {
                                    key: "edge-{edge.from_id}-{edge.to_id}",
                                    class: "swarm-edge",
                                    style: swarm_edge_style(from, to),
                                }
                            }
                        }

                        for node in snapshot.nodes.iter() {
                            button {
                                key: "{node.node.id}",
                                class: swarm_node_class(node, selected_id.as_deref() == Some(node.node.id.as_str())),
                                style: swarm_node_style(node),
                                aria_label: "{node.node.title}",
                                onclick: {
                                    let node_id = node.node.id.clone();
                                    move |_| on_select_node.call(node_id.clone())
                                },
                                div { class: "swarm-node-title", "{node.node.title}" }
                                div { class: "swarm-node-subtitle", "{node.node.subtitle}" }
                                if !node.node.detail.is_empty() {
                                    div { class: "swarm-node-detail", "{node.node.detail}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn swarm_node_style(node: &SwarmCanvasNode) -> String {
    format!(
        "left: {}px; top: {}px; width: {}px; min-height: {}px;",
        node.x, node.y, node.width, node.height
    )
}

fn swarm_edge_style(from: &SwarmCanvasNode, to: &SwarmCanvasNode) -> String {
    let start_x = from.x + from.width - 8.0;
    let start_y = from.y + from.height * 0.5;
    let end_x = to.x + 10.0;
    let end_y = to.y + to.height * 0.5;
    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let length = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx).to_degrees();
    format!(
        "left: {}px; top: {}px; width: {}px; transform: rotate({}deg);",
        start_x, start_y, length, angle
    )
}

fn swarm_node_class(node: &SwarmCanvasNode, selected: bool) -> String {
    let mut classes = vec!["swarm-node"];
    classes.push(match node.node.kind {
        SwarmNodeKind::Brain => "swarm-node-brain",
        SwarmNodeKind::Turn => "swarm-node-turn",
        SwarmNodeKind::Agent => "swarm-node-agent",
        SwarmNodeKind::Activity => "swarm-node-activity",
        SwarmNodeKind::Command => "swarm-node-command",
    });
    classes.push(match node.node.status {
        SwarmNodeStatus::Idle => "swarm-node-idle",
        SwarmNodeStatus::Queued => "swarm-node-queued",
        SwarmNodeStatus::Running => "swarm-node-running",
        SwarmNodeStatus::Waiting => "swarm-node-waiting",
        SwarmNodeStatus::Complete => "swarm-node-complete",
        SwarmNodeStatus::Failed => "swarm-node-failed",
        SwarmNodeStatus::Interrupted => "swarm-node-interrupted",
    });
    if selected {
        classes.push("swarm-node-selected");
    }
    classes.join(" ")
}

fn apply_payload_result(
    payload: Result<AccountsPayload, String>,
    mut accounts: Signal<Vec<AccountSummary>>,
    mut active_account_id: Signal<Option<String>>,
    mut notice: Signal<Option<String>>,
    _refreshed: bool,
) -> Result<(), String> {
    let payload = match payload {
        Ok(payload) => payload,
        Err(error) => {
            notice.set(Some(error.clone()));
            return Err(error);
        }
    };
    accounts.set(payload.accounts);
    active_account_id.set(payload.active_account_id);
    notice.set(None);
    Ok(())
}

fn account_title(account: &AccountSummary) -> String {
    match account.kind {
        AccountKind::ApiKey => account
            .masked_secret
            .clone()
            .unwrap_or_else(|| account.label.clone()),
        AccountKind::Chatgpt => account
            .email
            .clone()
            .unwrap_or_else(|| account.label.clone()),
    }
}

fn account_subtitle(account: &AccountSummary) -> String {
    match account.kind {
        AccountKind::ApiKey => "API".to_string(),
        AccountKind::Chatgpt => account
            .plan_type
            .clone()
            .map(titlecase_label)
            .unwrap_or_else(|| "Account".to_string()),
    }
}

fn titlecase_label(value: String) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return value;
    };

    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn usage_rails(snapshot: &RateLimitSnapshot, kind: &AccountKind, now_epoch: i64) -> Vec<UsageRail> {
    match kind {
        AccountKind::ApiKey => Vec::new(),
        AccountKind::Chatgpt => vec![
            UsageRail {
                percent: usage_left_percent(snapshot.primary_used_percent),
                reset_text: reset_countdown(snapshot.primary_resets_at.as_deref(), now_epoch),
            },
            UsageRail {
                percent: usage_left_percent(snapshot.secondary_used_percent),
                reset_text: reset_countdown(snapshot.secondary_resets_at.as_deref(), now_epoch),
            },
        ],
    }
}

fn usage_left_percent(used_percent: Option<f64>) -> f64 {
    (100.0 - used_percent.unwrap_or(0.0)).clamp(0.0, 100.0)
}

fn reset_countdown(reset_at: Option<&str>, now_epoch: i64) -> String {
    let Some(reset_at) = reset_at else {
        return "--:--:--:--".to_string();
    };

    let Some(reset_at) = reset_at.parse::<i64>().ok() else {
        return "--:--:--:--".to_string();
    };

    let remaining = reset_at.saturating_sub(now_epoch).max(0);
    let values = [
        remaining / 86_400,
        (remaining % 86_400) / 3_600,
        (remaining % 3_600) / 60,
        remaining % 60,
    ];
    let start_index = values
        .iter()
        .position(|value| *value != 0)
        .unwrap_or(values.len() - 1);

    values[start_index..]
        .iter()
        .map(|value| format!("{value:02}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn current_unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn submit_chat_prompt(
    services: Arc<AppServices>,
    mut prompt: Signal<String>,
    messages: Signal<Vec<MessageItem>>,
    inbox_items: Signal<Vec<InboxItem>>,
    swarm: Signal<SwarmProjection>,
    chat_busy: Signal<bool>,
    chat_thread_id: Signal<Option<String>>,
    active_turn_id: Signal<Option<String>>,
    mut attachments: Signal<Vec<ChatAttachment>>,
    selected_model: Signal<String>,
    selected_effort: Signal<String>,
    fast_mode: Signal<bool>,
    one_m_context: Signal<bool>,
    auto_drive_enabled: Signal<bool>,
    auto_drive_mode: Signal<AutoDriveMode>,
    auto_drive_started_at: Signal<Option<i64>>,
    auto_drive_completed_turns: Signal<u32>,
    suppress_next_auto_drive: Signal<bool>,
    auto_drive_runtime: Signal<String>,
    auto_drive_turns: Signal<String>,
    context_tokens: Signal<u64>,
    observed_context_window: Signal<Option<u64>>,
    codex_runtime_dialog: Signal<Option<String>>,
    notice: Signal<Option<String>>,
) {
    let user_message = prompt().trim().to_string();
    let pending_attachments = attachments();
    if (user_message.is_empty() && pending_attachments.is_empty()) || chat_busy() {
        return;
    }

    let prompt_to_send = prompt_text_for_submission(&user_message, &pending_attachments);
    let display_text = display_user_message(&user_message, &pending_attachments);
    prompt.set(String::new());
    attachments.set(Vec::new());

    submit_arbitrary_prompt(
        services,
        prompt_to_send,
        display_text,
        true,
        true,
        messages,
        inbox_items,
        swarm,
        chat_busy,
        chat_thread_id,
        active_turn_id,
        selected_model,
        selected_effort,
        fast_mode,
        one_m_context,
        auto_drive_enabled,
        auto_drive_mode,
        auto_drive_started_at,
        auto_drive_completed_turns,
        suppress_next_auto_drive,
        auto_drive_runtime,
        auto_drive_turns,
        context_tokens,
        observed_context_window,
        codex_runtime_dialog,
        notice,
    );
}

fn submit_arbitrary_prompt(
    services: Arc<AppServices>,
    prompt_to_send: String,
    display_text: String,
    add_user_message: bool,
    reset_auto_drive_cycle: bool,
    mut messages: Signal<Vec<MessageItem>>,
    inbox_items: Signal<Vec<InboxItem>>,
    mut swarm: Signal<SwarmProjection>,
    mut chat_busy: Signal<bool>,
    chat_thread_id: Signal<Option<String>>,
    mut active_turn_id: Signal<Option<String>>,
    selected_model: Signal<String>,
    selected_effort: Signal<String>,
    fast_mode: Signal<bool>,
    one_m_context: Signal<bool>,
    auto_drive_enabled: Signal<bool>,
    auto_drive_mode: Signal<AutoDriveMode>,
    mut auto_drive_started_at: Signal<Option<i64>>,
    mut auto_drive_completed_turns: Signal<u32>,
    mut suppress_next_auto_drive: Signal<bool>,
    auto_drive_runtime: Signal<String>,
    auto_drive_turns: Signal<String>,
    mut context_tokens: Signal<u64>,
    mut observed_context_window: Signal<Option<u64>>,
    mut codex_runtime_dialog: Signal<Option<String>>,
    mut notice: Signal<Option<String>>,
) {
    let pending_attachments = Vec::<ChatAttachment>::new();
    let current_thread_id = chat_thread_id();
    let selected_model_id = if selected_model().is_empty() {
        "gpt-5.4".to_string()
    } else {
        selected_model()
    };
    let long_context = one_m_context() && model_supports_long_context(&selected_model_id);
    let settings = ChatTurnSettings {
        model: selected_model_id,
        effort: Some(selected_effort()),
        service_tier: fast_mode().then(|| "fast".to_string()),
        long_context,
    };
    if current_thread_id.is_none() {
        context_tokens.set(0);
    }
    observed_context_window.set(Some(if settings.long_context {
        LONG_CONTEXT_WINDOW
    } else {
        DEFAULT_CONTEXT_WINDOW
    }));
    if auto_drive_enabled() && reset_auto_drive_cycle {
        auto_drive_started_at.set(Some(current_unix_timestamp()));
        auto_drive_completed_turns.set(0);
    } else if !auto_drive_enabled() {
        auto_drive_started_at.set(None);
        auto_drive_completed_turns.set(0);
    }
    suppress_next_auto_drive.set(false);
    active_turn_id.set(None);
    codex_runtime_dialog.set(None);
    notice.set(None);
    chat_busy.set(true);
    if add_user_message {
        messages.with_mut(|items| {
            items.push(MessageItem {
                id: local_message_id("user"),
                kind: ChatMessageKind::User,
                title: None,
                agent_label: None,
                text: display_text,
                details: None,
                expanded: false,
                complete: true,
            });
        });
    } else {
        append_or_replace_status(messages, &display_text);
    }
    swarm.with_mut(|projection| {
        projection.start_turn(&prompt_to_send, &pending_attachments, &settings);
    });

    spawn(async move {
        let _ = document::eval(
            r#"
            const el = document.getElementById('messages-panel');
            if (el) {
              el.dataset.followOutput = 'true';
              el.scrollTop = el.scrollHeight;
            }
            "#,
        )
        .await;
    });

    spawn(async move {
        let stream = services
            .runtime
            .stream_chat_prompt(
                services.accounts.clone(),
                current_thread_id,
                prompt_to_send,
                pending_attachments,
                settings,
            )
            .await;

        match stream {
            Ok(mut receiver) => {
                while let Some(event) = receiver.recv().await {
                    apply_chat_stream_event(
                        event,
                        messages,
                        inbox_items,
                        swarm,
                        chat_busy,
                        chat_thread_id,
                        active_turn_id,
                        services.clone(),
                        selected_model,
                        selected_effort,
                        fast_mode,
                        one_m_context,
                        auto_drive_enabled,
                        auto_drive_mode,
                        auto_drive_started_at,
                        auto_drive_completed_turns,
                        suppress_next_auto_drive,
                        auto_drive_runtime,
                        auto_drive_turns,
                        context_tokens,
                        observed_context_window,
                        codex_runtime_dialog,
                        notice,
                    );
                }
            }
            Err(error) => {
                notice.set(Some(error.clone()));
                active_turn_id.set(None);
                swarm.with_mut(|projection| {
                    projection.apply_chat_event(&ChatStreamEvent::Error {
                        message: error.clone(),
                    });
                    projection.apply_chat_event(&ChatStreamEvent::Completed);
                });
                messages.with_mut(|items| {
                    items.push(MessageItem {
                        id: local_message_id("error"),
                        kind: ChatMessageKind::Status,
                        title: Some("Error".to_string()),
                        agent_label: None,
                        text: error,
                        details: None,
                        expanded: false,
                        complete: true,
                    });
                });
                chat_busy.set(false);
            }
        }
    });
}

fn steer_chat_prompt(
    services: Arc<AppServices>,
    mut prompt: Signal<String>,
    messages: Signal<Vec<MessageItem>>,
    chat_thread_id: Signal<Option<String>>,
    active_turn_id: Signal<Option<String>>,
    mut attachments: Signal<Vec<ChatAttachment>>,
    mut notice: Signal<Option<String>>,
) {
    let user_message = prompt().trim().to_string();
    let pending_attachments = attachments();
    if user_message.is_empty() && pending_attachments.is_empty() {
        return;
    }

    let Some(thread_id) = chat_thread_id() else {
        notice.set(Some(
            "No active thread is available to steer yet.".to_string(),
        ));
        return;
    };
    let Some(turn_id) = active_turn_id() else {
        notice.set(Some(
            "No active turn is available to steer right now.".to_string(),
        ));
        return;
    };

    let prompt_to_send = prompt_text_for_submission(&user_message, &pending_attachments);
    let display_text = display_user_message(&user_message, &pending_attachments);
    prompt.set(String::new());
    attachments.set(Vec::new());

    steer_arbitrary_prompt(
        services,
        prompt_to_send,
        display_text,
        messages,
        Some(thread_id),
        Some(turn_id),
        notice,
    );
}

fn steer_arbitrary_prompt(
    services: Arc<AppServices>,
    prompt_to_send: String,
    display_text: String,
    mut messages: Signal<Vec<MessageItem>>,
    thread_id: Option<String>,
    turn_id: Option<String>,
    mut notice: Signal<Option<String>>,
) {
    let Some(thread_id) = thread_id else {
        notice.set(Some(
            "No active thread is available to steer yet.".to_string(),
        ));
        return;
    };
    let Some(turn_id) = turn_id else {
        notice.set(Some(
            "No active turn is available to steer right now.".to_string(),
        ));
        return;
    };

    let pending_attachments = Vec::<ChatAttachment>::new();
    notice.set(None);
    messages.with_mut(|items| {
        items.push(MessageItem {
            id: local_message_id("user-steer"),
            kind: ChatMessageKind::User,
            title: None,
            agent_label: None,
            text: display_text,
            details: None,
            expanded: false,
            complete: true,
        });
    });

    spawn(async move {
        let _ = document::eval(
            r#"
            const el = document.getElementById('messages-panel');
            if (el) {
              el.dataset.followOutput = 'true';
              el.scrollTop = el.scrollHeight;
            }
            "#,
        )
        .await;
    });

    spawn(async move {
        if let Err(error) = services
            .runtime
            .steer_turn(&thread_id, &turn_id, prompt_to_send, pending_attachments)
            .await
        {
            notice.set(Some(error.clone()));
            messages.with_mut(|items| {
                items.push(MessageItem {
                    id: local_message_id("error"),
                    kind: ChatMessageKind::Status,
                    title: Some("Error".to_string()),
                    agent_label: None,
                    text: error,
                    details: None,
                    expanded: false,
                    complete: true,
                });
            });
        }
    });
}

fn push_auto_drive_status(mut messages: Signal<Vec<MessageItem>>, text: &str) {
    messages.with_mut(|items| {
        items.push(MessageItem {
            id: local_message_id("auto-drive"),
            kind: ChatMessageKind::Activity,
            title: Some("Auto Drive".to_string()),
            agent_label: None,
            text: text.to_string(),
            details: None,
            expanded: false,
            complete: true,
        });
    });
}

fn maybe_continue_auto_drive(
    services: Arc<AppServices>,
    messages: Signal<Vec<MessageItem>>,
    inbox_items: Signal<Vec<InboxItem>>,
    swarm: Signal<SwarmProjection>,
    chat_busy: Signal<bool>,
    chat_thread_id: Signal<Option<String>>,
    active_turn_id: Signal<Option<String>>,
    selected_model: Signal<String>,
    selected_effort: Signal<String>,
    fast_mode: Signal<bool>,
    one_m_context: Signal<bool>,
    auto_drive_enabled: Signal<bool>,
    auto_drive_mode: Signal<AutoDriveMode>,
    mut auto_drive_started_at: Signal<Option<i64>>,
    mut auto_drive_completed_turns: Signal<u32>,
    suppress_next_auto_drive: Signal<bool>,
    auto_drive_runtime: Signal<String>,
    auto_drive_turns: Signal<String>,
    context_tokens: Signal<u64>,
    observed_context_window: Signal<Option<u64>>,
    codex_runtime_dialog: Signal<Option<String>>,
    mut notice: Signal<Option<String>>,
) {
    let snapshot = messages.read().clone();
    if !auto_drive_enabled() {
        auto_drive_started_at.set(None);
        auto_drive_completed_turns.set(0);
        return;
    }

    if chat_thread_id().is_none() {
        auto_drive_started_at.set(None);
        auto_drive_completed_turns.set(0);
        return;
    }

    let now = current_unix_timestamp();
    let started_at = auto_drive_started_at().unwrap_or(now);
    if auto_drive_started_at().is_none() {
        auto_drive_started_at.set(Some(started_at));
    }

    let completed_turns = auto_drive_completed_turns().saturating_add(1);
    auto_drive_completed_turns.set(completed_turns);

    if snapshot.iter().rev().any(|message| {
        message.kind == ChatMessageKind::Status
            && message.complete
            && message.title.as_deref() == Some("Error")
    }) {
        push_auto_drive_status(messages, "Stopped: the last turn ended with an error.");
        return;
    }

    if let Some(limit) = optional_limit_value(&auto_drive_turns()) {
        if completed_turns >= limit {
            push_auto_drive_status(messages, "Stopped: reached the Auto Drive turn limit.");
            return;
        }
    }

    if let Some(limit_hours) = optional_limit_value(&auto_drive_runtime()) {
        let runtime_limit = i64::from(limit_hours) * 3600;
        if now.saturating_sub(started_at) >= runtime_limit {
            push_auto_drive_status(messages, "Stopped: reached the Auto Drive runtime limit.");
            return;
        }
    }

    let recent_assistants = last_completed_assistant_messages(&snapshot, 2);
    let last_assistant = recent_assistants.first().cloned();
    let Some(last_assistant) = last_assistant else {
        push_auto_drive_status(
            messages,
            "Stopped: there is no completed assistant output to build on yet.",
        );
        return;
    };

    let stop_reason = extract_auto_drive_stop_reason(&last_assistant.text, true);
    let mode = auto_drive_mode();

    if mode == AutoDriveMode::Completion {
        if let Some(reason) = stop_reason {
            push_auto_drive_status(messages, &format!("Stopped: {reason}."));
            return;
        }
    }

    let last_normalized = normalized_auto_drive_text(&last_assistant.text);
    let previous_normalized = recent_assistants
        .get(1)
        .map(|message| normalized_auto_drive_text(&message.text));
    let stalled = !last_normalized.is_empty()
        && previous_normalized
            .as_ref()
            .is_some_and(|previous| previous == &last_normalized);
    let meta_loop = recent_assistants
        .iter()
        .take(2)
        .all(|message| is_auto_drive_meta_response(&message.text));
    let replay_loop = appears_to_replay_existing_text(&last_assistant.text, &snapshot);
    let creative_writing = last_completed_user_message(&snapshot)
        .as_deref()
        .is_some_and(is_creative_writing_request);
    let open_questions = inbox_items().iter().filter(|item| !item.resolved).count();
    let completion_signal = stop_reason.is_some();
    let prompt = auto_drive_prompt(
        &last_assistant.text,
        open_questions,
        stalled,
        completion_signal,
        meta_loop,
        replay_loop,
        creative_writing,
        mode,
        completed_turns,
    );
    let status_text = if mode == AutoDriveMode::OpenEnded && replay_loop && creative_writing {
        "Detected replay. Forcing continuation from the latest frontier..."
    } else if mode == AutoDriveMode::OpenEnded && meta_loop && creative_writing {
        "Switching from review to direct story output..."
    } else if mode == AutoDriveMode::OpenEnded && meta_loop {
        "Switching from review to direct execution..."
    } else if mode == AutoDriveMode::OpenEnded && creative_writing {
        "Continuing the story from the latest frontier..."
    } else if mode == AutoDriveMode::OpenEnded && completion_signal {
        "Pushing past the first completed draft..."
    } else if stalled {
        "Replanning from the latest state..."
    } else {
        "Reviewing the latest work and continuing..."
    };

    push_auto_drive_status(messages, status_text);
    notice.set(None);
    submit_arbitrary_prompt(
        services,
        prompt,
        status_text.to_string(),
        false,
        false,
        messages,
        inbox_items,
        swarm,
        chat_busy,
        chat_thread_id,
        active_turn_id,
        selected_model,
        selected_effort,
        fast_mode,
        one_m_context,
        auto_drive_enabled,
        auto_drive_mode,
        auto_drive_started_at,
        auto_drive_completed_turns,
        suppress_next_auto_drive,
        auto_drive_runtime,
        auto_drive_turns,
        context_tokens,
        observed_context_window,
        codex_runtime_dialog,
        notice,
    );
}

fn apply_chat_stream_event(
    event: ChatStreamEvent,
    mut messages: Signal<Vec<MessageItem>>,
    inbox_items: Signal<Vec<InboxItem>>,
    mut swarm: Signal<SwarmProjection>,
    mut chat_busy: Signal<bool>,
    mut chat_thread_id: Signal<Option<String>>,
    mut active_turn_id: Signal<Option<String>>,
    services: Arc<AppServices>,
    selected_model: Signal<String>,
    selected_effort: Signal<String>,
    fast_mode: Signal<bool>,
    one_m_context: Signal<bool>,
    auto_drive_enabled: Signal<bool>,
    auto_drive_mode: Signal<AutoDriveMode>,
    auto_drive_started_at: Signal<Option<i64>>,
    auto_drive_completed_turns: Signal<u32>,
    mut suppress_next_auto_drive: Signal<bool>,
    auto_drive_runtime: Signal<String>,
    auto_drive_turns: Signal<String>,
    mut context_tokens: Signal<u64>,
    mut observed_context_window: Signal<Option<u64>>,
    mut codex_runtime_dialog: Signal<Option<String>>,
    mut notice: Signal<Option<String>>,
) {
    swarm.with_mut(|projection| projection.apply_chat_event(&event));
    match event {
        ChatStreamEvent::CodexRuntimeDialog { message } => {
            codex_runtime_dialog.set(message);
        }
        ChatStreamEvent::ThreadReady { thread_id } => {
            chat_thread_id.set(Some(thread_id));
        }
        ChatStreamEvent::TurnStarted { turn_id } => {
            active_turn_id.set(Some(turn_id));
        }
        ChatStreamEvent::Status { message } => {
            append_or_replace_status(messages, &message);
        }
        ChatStreamEvent::Activity {
            item_id,
            title,
            detail,
            agent_label,
            complete,
        } => {
            complete_active_status_messages(messages);
            upsert_activity_message(
                messages,
                &item_id,
                &title,
                agent_label.as_deref(),
                &detail,
                complete,
            );
        }
        ChatStreamEvent::AgentThread { .. } => {}
        ChatStreamEvent::CollabTool {
            item_id,
            title,
            detail,
            agent_label,
            complete,
            ..
        } => {
            complete_active_status_messages(messages);
            upsert_activity_message(
                messages,
                &item_id,
                &title,
                agent_label.as_deref(),
                &detail,
                complete,
            );
        }
        ChatStreamEvent::TokenUsage {
            context_tokens: total_tokens,
            session_total_tokens: _,
            context_window,
        } => {
            context_tokens.set(total_tokens);
            observed_context_window.set(Some(context_window));
        }
        ChatStreamEvent::AssistantDelta {
            item_id,
            title,
            delta,
        } => {
            complete_active_status_messages(messages);
            append_chat_delta(
                messages,
                &item_id,
                ChatMessageKind::Assistant,
                title.as_deref(),
                title.as_deref(),
                &delta,
            );
            if let Some(message) = messages
                .read()
                .iter()
                .find(|item| item.id == item_id)
                .cloned()
            {
                upsert_inbox_items_from_message(inbox_items, &message, false);
            }
        }
        ChatStreamEvent::ReasoningDelta {
            item_id,
            title,
            delta,
        } => {
            complete_active_status_messages(messages);
            append_chat_delta(
                messages,
                &item_id,
                ChatMessageKind::Reasoning,
                title.as_deref().or(Some("Thinking")),
                title.as_deref(),
                &delta,
            );
        }
        ChatStreamEvent::CommandStarted {
            item_id,
            title,
            command,
        } => {
            complete_active_status_messages(messages);
            ensure_command_message(messages, &item_id, title.as_deref(), &command);
        }
        ChatStreamEvent::CommandDelta { item_id, delta } => {
            complete_active_status_messages(messages);
            append_command_output(messages, &item_id, &delta);
        }
        ChatStreamEvent::ItemDone { item_id } => {
            messages.with_mut(|items| {
                if let Some(item) = items.iter_mut().find(|item| item.id == item_id) {
                    item.complete = true;
                }
            });
            if let Some(message) = messages
                .read()
                .iter()
                .find(|item| item.id == item_id)
                .cloned()
            {
                upsert_inbox_items_from_message(inbox_items, &message, true);
            }
        }
        ChatStreamEvent::Error { message } => {
            codex_runtime_dialog.set(None);
            complete_active_status_messages(messages);
            notice.set(Some(message.clone()));
            messages.with_mut(|items| {
                items.push(MessageItem {
                    id: local_message_id("error"),
                    kind: ChatMessageKind::Status,
                    title: Some("Error".to_string()),
                    agent_label: None,
                    text: message,
                    details: None,
                    expanded: false,
                    complete: true,
                });
            });
        }
        ChatStreamEvent::Completed => {
            codex_runtime_dialog.set(None);
            complete_active_status_messages(messages);
            active_turn_id.set(None);
            chat_busy.set(false);
            if suppress_next_auto_drive() {
                suppress_next_auto_drive.set(false);
                push_auto_drive_status(messages, "Stopped by user.");
                return;
            }
            maybe_continue_auto_drive(
                services,
                messages,
                inbox_items,
                swarm,
                chat_busy,
                chat_thread_id,
                active_turn_id,
                selected_model,
                selected_effort,
                fast_mode,
                one_m_context,
                auto_drive_enabled,
                auto_drive_mode,
                auto_drive_started_at,
                auto_drive_completed_turns,
                suppress_next_auto_drive,
                auto_drive_runtime,
                auto_drive_turns,
                context_tokens,
                observed_context_window,
                codex_runtime_dialog,
                notice,
            );
        }
    }
}

fn append_or_replace_status(mut messages: Signal<Vec<MessageItem>>, text: &str) {
    messages.with_mut(|items| {
        if let Some(item) = items
            .iter_mut()
            .rev()
            .find(|item| item.kind == ChatMessageKind::Status && !item.complete)
        {
            item.text = text.to_string();
            return;
        }

        items.push(MessageItem {
            id: local_message_id("status"),
            kind: ChatMessageKind::Status,
            title: None,
            agent_label: None,
            text: text.to_string(),
            details: None,
            expanded: false,
            complete: false,
        });
    });
}

fn complete_active_status_messages(mut messages: Signal<Vec<MessageItem>>) {
    messages.with_mut(|items| {
        items.retain(|item| !(item.kind == ChatMessageKind::Status && !item.complete));
    });
}

fn ensure_command_message(
    mut messages: Signal<Vec<MessageItem>>,
    id: &str,
    title: Option<&str>,
    command: &str,
) {
    messages.with_mut(|items| {
        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
            item.kind = ChatMessageKind::Command;
            item.title = title.map(str::to_string);
            item.agent_label = title.map(str::to_string);
            item.text = format!("$ {command}");
            if item.details.is_none() {
                item.details = Some(String::new());
            }
            return;
        }

        items.push(MessageItem {
            id: id.to_string(),
            kind: ChatMessageKind::Command,
            title: title.map(str::to_string),
            agent_label: title.map(str::to_string),
            text: format!("$ {command}"),
            details: Some(String::new()),
            expanded: false,
            complete: false,
        });
    });
}

fn upsert_activity_message(
    mut messages: Signal<Vec<MessageItem>>,
    id: &str,
    title: &str,
    agent_label: Option<&str>,
    detail: &str,
    complete: bool,
) {
    messages.with_mut(|items| {
        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
            item.kind = ChatMessageKind::Activity;
            item.title = Some(title.to_string());
            item.agent_label = agent_label.map(str::to_string);
            item.text = detail.to_string();
            item.complete = complete;
            return;
        }

        items.push(MessageItem {
            id: id.to_string(),
            kind: ChatMessageKind::Activity,
            title: Some(title.to_string()),
            agent_label: agent_label.map(str::to_string),
            text: detail.to_string(),
            details: None,
            expanded: false,
            complete,
        });
    });
}

fn append_command_output(mut messages: Signal<Vec<MessageItem>>, id: &str, delta: &str) {
    messages.with_mut(|items| {
        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
            let details = item.details.get_or_insert_with(String::new);
            details.push_str(delta);
        }
    });
}

fn append_chat_delta(
    mut messages: Signal<Vec<MessageItem>>,
    id: &str,
    kind: ChatMessageKind,
    title: Option<&str>,
    agent_label: Option<&str>,
    delta: &str,
) {
    messages.with_mut(|items| {
        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
            if item.title.is_none() {
                item.title = title.map(str::to_string);
            }
            if item.agent_label.is_none() {
                item.agent_label = agent_label.map(str::to_string);
            }
            item.text.push_str(delta);
            return;
        }

        items.push(MessageItem {
            id: id.to_string(),
            kind,
            title: title.map(str::to_string),
            agent_label: agent_label.map(str::to_string),
            text: delta.to_string(),
            details: None,
            expanded: false,
            complete: false,
        });
    });
}

fn toggle_command_message(mut messages: Signal<Vec<MessageItem>>, id: &str) {
    messages.with_mut(|items| {
        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
            if item.kind == ChatMessageKind::Command
                && item
                    .details
                    .as_ref()
                    .is_some_and(|details| !details.is_empty())
            {
                item.expanded = !item.expanded;
            }
        }
    });
}

fn chat_message_class(message: &MessageItem) -> String {
    let mut classes = vec!["message"];
    classes.push(match message.kind {
        ChatMessageKind::User => "message-user",
        ChatMessageKind::Assistant => "message-assistant",
        ChatMessageKind::Reasoning => "message-reasoning",
        ChatMessageKind::Activity => "message-activity",
        ChatMessageKind::Command => "message-command",
        ChatMessageKind::Status => "message-status",
    });
    if message.agent_label.is_some() {
        classes.push("message-agent");
    }
    classes.join(" ")
}

fn local_message_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{prefix}-{nanos:x}")
}
