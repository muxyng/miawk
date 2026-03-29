use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::Mutex,
};

use super::codex::ensure_managed_codex;

const ACCOUNTS_FILE: &str = "accounts.json";
const ACCOUNT_ROOT_DIR: &str = "accounts";

#[derive(Debug, Clone)]
pub struct ResolvedCodexAccount {
    pub id: String,
    pub codex_home: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    ApiKey,
    Chatgpt,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RateLimitSnapshot {
    pub requests_limit: Option<String>,
    pub requests_remaining: Option<String>,
    pub requests_reset: Option<String>,
    pub tokens_limit: Option<String>,
    pub tokens_remaining: Option<String>,
    pub tokens_reset: Option<String>,
    pub primary_used_percent: Option<f64>,
    pub primary_window_minutes: Option<i64>,
    pub primary_resets_at: Option<String>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<String>,
    pub credits_balance: Option<String>,
    pub credits_unlimited: Option<bool>,
    pub checked_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAccount {
    pub id: String,
    pub kind: AccountKind,
    pub label: String,
    pub codex_home: String,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub subscription_active_until: Option<String>,
    pub rate_limits: RateLimitSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AccountsStore {
    pub active_account_id: Option<String>,
    pub accounts: Vec<StoredAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountSummary {
    pub id: String,
    pub kind: AccountKind,
    pub label: String,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub subscription_active_until: Option<String>,
    pub masked_secret: Option<String>,
    pub rate_limits: RateLimitSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountsPayload {
    pub active_account_id: Option<String>,
    pub accounts: Vec<AccountSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceAuthStart {
    pub pending_id: String,
    pub verification_uri: String,
    pub user_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeviceAuthPoll {
    Pending {
        pending_id: String,
        verification_uri: String,
        user_code: String,
    },
    Complete {
        account: AccountSummary,
        payload: AccountsPayload,
    },
}

#[derive(Default)]
pub struct AccountsService {
    pending_device_auth: Arc<Mutex<HashMap<String, PendingDeviceAuth>>>,
    data_dir: PathBuf,
}

struct PendingDeviceAuth {
    account_id: String,
    codex_home: PathBuf,
    verification_uri: String,
    user_code: String,
    child: Child,
}

#[derive(Debug, Deserialize)]
struct AuthFile {
    #[serde(rename = "auth_mode")]
    auth_mode: Option<String>,
    #[serde(rename = "OPENAI_API_KEY")]
    openai_api_key: Option<String>,
    tokens: Option<AuthTokens>,
}

#[derive(Debug, Deserialize)]
struct AuthTokens {
    id_token: Option<String>,
    access_token: Option<String>,
    account_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/auth")]
    auth: Option<AuthClaims>,
}

#[derive(Debug, Deserialize)]
struct AuthClaims {
    chatgpt_plan_type: Option<String>,
    chatgpt_subscription_active_until: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatGptUsagePayload {
    plan_type: Option<String>,
    rate_limit: Option<ChatGptUsageLimit>,
    credits: Option<ChatGptCredits>,
}

#[derive(Debug, Deserialize)]
struct ChatGptUsageLimit {
    primary_window: Option<ChatGptUsageWindow>,
    secondary_window: Option<ChatGptUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct ChatGptUsageWindow {
    used_percent: f64,
    limit_window_seconds: Option<i64>,
    reset_after_seconds: Option<i64>,
    reset_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ChatGptCredits {
    unlimited: Option<bool>,
    balance: Option<String>,
}

impl AccountsService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            pending_device_auth: Arc::default(),
            data_dir,
        }
    }

    pub async fn list_accounts(&self) -> Result<AccountsPayload, String> {
        let mut store = self.read_store()?;
        if self.reconcile_store_with_account_homes(&mut store)? {
            self.write_store(&store)?;
        }
        Ok(payload_from_store(&store))
    }

    pub async fn refresh_all_account_limits(&self) -> Result<AccountsPayload, String> {
        let mut store = self.read_store()?;
        if self.reconcile_store_with_account_homes(&mut store)? {
            self.write_store(&store)?;
        }
        for account in &mut store.accounts {
            refresh_limits_for_account(account).await?;
        }
        self.write_store(&store)?;
        Ok(payload_from_store(&store))
    }

    pub async fn remove_account(&self, account_id: String) -> Result<AccountsPayload, String> {
        let mut store = self.read_store()?;
        if let Some(account) = store
            .accounts
            .iter()
            .find(|entry| entry.id == account_id)
            .cloned()
        {
            let _ = fs::remove_dir_all(account.codex_home);
        }
        store.accounts.retain(|entry| entry.id != account_id);
        if store.active_account_id.as_deref() == Some(account_id.as_str()) {
            store.active_account_id = store.accounts.first().map(|entry| entry.id.clone());
        }
        self.write_store(&store)?;
        Ok(payload_from_store(&store))
    }

    pub async fn add_api_key_account(&self, api_key: String) -> Result<AccountsPayload, String> {
        let account_id = new_id();
        let codex_home = self.account_codex_home(&account_id);
        fs::create_dir_all(&codex_home).map_err(|error| error.to_string())?;
        let installation = ensure_managed_codex(&self.data_dir).await?;

        let mut child = Command::new(&installation.codex_bin)
            .arg("login")
            .arg("--with-api-key")
            .env("CODEX_HOME", &codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("failed to run codex login: {error}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(format!("{}\n", api_key.trim()).as_bytes())
                .await
                .map_err(|error| format!("failed to write API key to codex login: {error}"))?;
        }

        let status = child
            .wait()
            .await
            .map_err(|error| format!("failed to wait for codex login: {error}"))?;
        if !status.success() {
            return Err("codex login failed while linking the API key".to_string());
        }

        let mut account = load_account_from_codex_home(account_id, codex_home)?;
        refresh_limits_for_account(&mut account).await?;

        let mut store = self.read_store()?;
        store.active_account_id = Some(account.id.clone());
        store.accounts.retain(|entry| entry.id != account.id);
        store.accounts.insert(0, account);
        self.write_store(&store)?;
        Ok(payload_from_store(&store))
    }

    pub async fn start_chatgpt_account_link(&self) -> Result<DeviceAuthStart, String> {
        let account_id = new_id();
        let codex_home = self.account_codex_home(&account_id);
        fs::create_dir_all(&codex_home).map_err(|error| error.to_string())?;
        let installation = ensure_managed_codex(&self.data_dir).await?;

        let mut child = Command::new(&installation.codex_bin)
            .arg("login")
            .arg("--device-auth")
            .env("CODEX_HOME", &codex_home)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("failed to start Codex ChatGPT login: {error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture device auth output".to_string())?;
        let mut reader = BufReader::new(stdout).lines();
        let mut verification_uri = None;
        let mut user_code = None;

        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|error| format!("failed to read device auth output: {error}"))?
        {
            let clean = strip_ansi(&line);
            if verification_uri.is_none() {
                verification_uri = clean
                    .split_whitespace()
                    .find(|part| part.starts_with("https://"))
                    .map(ToOwned::to_owned);
            }
            if user_code.is_none() {
                user_code = clean
                    .split_whitespace()
                    .find(|part| is_device_code(part))
                    .map(ToOwned::to_owned);
            }
            if verification_uri.is_some() && user_code.is_some() {
                break;
            }
        }

        let verification_uri = verification_uri
            .ok_or_else(|| "failed to read ChatGPT device-auth URL from Codex".to_string())?;
        let user_code = user_code
            .ok_or_else(|| "failed to read ChatGPT device-auth code from Codex".to_string())?;

        let pending_id = new_id();
        self.pending_device_auth.lock().await.insert(
            pending_id.clone(),
            PendingDeviceAuth {
                account_id,
                codex_home,
                verification_uri: verification_uri.clone(),
                user_code: user_code.clone(),
                child,
            },
        );

        Ok(DeviceAuthStart {
            pending_id,
            verification_uri,
            user_code,
        })
    }

    pub async fn poll_chatgpt_account_link(
        &self,
        pending_id: String,
    ) -> Result<DeviceAuthPoll, String> {
        let pending_status = {
            let mut pending = self.pending_device_auth.lock().await;
            let Some(entry) = pending.get_mut(&pending_id) else {
                return Err("device auth session not found".to_string());
            };

            if auth_file_ready(&entry.codex_home)? {
                let mut ready = pending
                    .remove(&pending_id)
                    .ok_or_else(|| "device auth session ended unexpectedly".to_string())?;
                let _ = ready.child.start_kill();
                ready.child.stdout.take();
                Some(ready)
            } else {
                match entry.child.try_wait() {
                    Ok(None) => {
                        return Ok(DeviceAuthPoll::Pending {
                            pending_id,
                            verification_uri: entry.verification_uri.clone(),
                            user_code: entry.user_code.clone(),
                        });
                    }
                    Ok(Some(status)) if status.success() => pending.remove(&pending_id),
                    Ok(Some(status)) => {
                        let _ = pending.remove(&pending_id);
                        return Err(format!("ChatGPT device auth failed with status {status}"));
                    }
                    Err(error) => {
                        let _ = pending.remove(&pending_id);
                        return Err(format!("failed to check device auth status: {error}"));
                    }
                }
            }
        };

        let pending =
            pending_status.ok_or_else(|| "device auth session ended unexpectedly".to_string())?;
        let mut account = load_account_from_codex_home(pending.account_id, pending.codex_home)?;
        refresh_limits_for_account(&mut account).await?;

        let mut store = self.read_store()?;
        store.active_account_id = Some(account.id.clone());
        store.accounts.retain(|entry| entry.id != account.id);
        store.accounts.insert(0, account.clone());
        self.write_store(&store)?;

        Ok(DeviceAuthPoll::Complete {
            account: account_summary(&account),
            payload: payload_from_store(&store),
        })
    }

    pub async fn resolve_runtime_account(&self) -> Result<Option<ResolvedCodexAccount>, String> {
        let mut store = self.read_store()?;
        if self.reconcile_store_with_account_homes(&mut store)? {
            self.write_store(&store)?;
        }

        let mut fallback = None::<ResolvedCodexAccount>;
        let mut fallback_score = i64::MIN;
        let mut degraded = None::<ResolvedCodexAccount>;

        for account in &store.accounts {
            let unavailable_reason = account_unavailable_reason(account)?;
            let resolved = ResolvedCodexAccount {
                id: account.id.clone(),
                codex_home: PathBuf::from(&account.codex_home),
            };

            if degraded.is_none() {
                degraded = Some(resolved.clone());
            }

            if store.active_account_id.as_deref() == Some(account.id.as_str())
                && unavailable_reason.is_some()
            {
                degraded = Some(resolved.clone());
            }

            let Some(score) = account_runtime_score(account)? else {
                continue;
            };

            if store.active_account_id.as_deref() == Some(account.id.as_str()) {
                return Ok(Some(resolved));
            }

            if score > fallback_score {
                fallback_score = score;
                fallback = Some(resolved);
            }
        }

        if let Some(selected) = fallback.clone() {
            if store.active_account_id.as_deref() != Some(selected.id.as_str()) {
                store.active_account_id = Some(selected.id.clone());
                self.write_store(&store)?;
            }
            return Ok(Some(selected));
        }

        if let Some(selected) = degraded {
            if store.active_account_id.as_deref() != Some(selected.id.as_str()) {
                store.active_account_id = Some(selected.id.clone());
                self.write_store(&store)?;
            }
            return Ok(Some(selected));
        }

        Ok(None)
    }

    fn read_store(&self) -> Result<AccountsStore, String> {
        let store_path = self.store_path();
        if !store_path.exists() {
            return Ok(AccountsStore::default());
        }
        let raw = fs::read_to_string(&store_path).map_err(|error| {
            format!(
                "failed to read accounts store {}: {error}",
                store_path.display()
            )
        })?;
        let mut store: AccountsStore = serde_json::from_str(&raw)
            .map_err(|error| format!("failed to parse accounts store: {error}"))?;
        if self.migrate_store_account_paths(&mut store) {
            self.write_store(&store)?;
        }
        Ok(store)
    }

    fn write_store(&self, store: &AccountsStore) -> Result<(), String> {
        let store_path = self.store_path();
        if let Some(parent) = store_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create accounts directory: {error}"))?;
        }
        let raw = serde_json::to_string_pretty(store)
            .map_err(|error| format!("failed to encode accounts store: {error}"))?;
        fs::write(&store_path, raw)
            .map_err(|error| format!("failed to write accounts store: {error}"))
    }

    fn store_path(&self) -> PathBuf {
        self.data_dir.join(ACCOUNTS_FILE)
    }

    fn account_root_dir(&self) -> PathBuf {
        self.data_dir.join(ACCOUNT_ROOT_DIR)
    }

    fn account_codex_home(&self, account_id: &str) -> PathBuf {
        self.account_root_dir().join(account_id).join("codex-home")
    }

    fn legacy_data_dir(&self) -> Option<PathBuf> {
        let current_name = self.data_dir.file_name()?.to_str()?;
        if current_name != "com.melani.miawk" {
            return None;
        }

        Some(self.data_dir.parent()?.join("com.melani.rsc"))
    }

    fn legacy_account_codex_home(&self, account_id: &str) -> Option<PathBuf> {
        Some(
            self.legacy_data_dir()?
                .join(ACCOUNT_ROOT_DIR)
                .join(account_id)
                .join("codex-home"),
        )
    }

    fn candidate_codex_homes(&self, account: &StoredAccount) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        let stored = PathBuf::from(&account.codex_home);
        candidates.push(stored);

        let current = self.account_codex_home(&account.id);
        if !candidates.iter().any(|path| path == &current) {
            candidates.push(current);
        }

        if let Some(legacy) = self.legacy_account_codex_home(&account.id) {
            if !candidates.iter().any(|path| path == &legacy) {
                candidates.push(legacy);
            }
        }

        candidates
    }

    fn migrate_store_account_paths(&self, store: &mut AccountsStore) -> bool {
        let mut changed = false;

        for account in &mut store.accounts {
            let current = self.account_codex_home(&account.id);
            let Some(candidate) = self
                .candidate_codex_homes(account)
                .into_iter()
                .find(|path| auth_file_usable(path))
            else {
                continue;
            };

            let resolved_home = if candidate == current {
                current
            } else {
                let mut current_usable = auth_file_usable(&current);
                if !current_usable {
                    let current_auth = current.join("auth.json");
                    if let Some(parent) = current.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if !current_auth.exists() {
                        let _ = fs::rename(&candidate, &current);
                        current_usable = auth_file_usable(&current);
                    }
                }

                if current_usable { current } else { candidate }
            };

            if account.codex_home != resolved_home.to_string_lossy() {
                account.codex_home = resolved_home.to_string_lossy().into_owned();
                changed = true;
            }
        }

        changed
    }

    fn account_has_viable_home(&self, account: &StoredAccount) -> bool {
        self.candidate_codex_homes(account)
            .into_iter()
            .any(|path| auth_file_usable(&path))
    }

    fn reconcile_store_with_account_homes(
        &self,
        store: &mut AccountsStore,
    ) -> Result<bool, String> {
        let mut changed = self.migrate_store_account_paths(store);

        let original_len = store.accounts.len();
        store.accounts.retain(|account| self.account_has_viable_home(account));
        if store.accounts.len() != original_len {
            changed = true;
        }

        let root = self.account_root_dir();
        if !root.exists() {
            if store.active_account_id.as_ref().is_some_and(|active_id| {
                store
                    .accounts
                    .iter()
                    .all(|account| &account.id != active_id)
            }) {
                store.active_account_id = None;
                changed = true;
            }

            if store.active_account_id.is_none() && !store.accounts.is_empty() {
                store.active_account_id = store.accounts.first().map(|account| account.id.clone());
                changed = true;
            }

            return Ok(changed);
        }

        for entry in fs::read_dir(&root).map_err(|error| {
            format!(
                "failed to read accounts directory {}: {error}",
                root.display()
            )
        })? {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to inspect accounts directory {}: {error}",
                    root.display()
                )
            })?;
            if !entry
                .file_type()
                .map_err(|error| format!("failed to inspect account entry: {error}"))?
                .is_dir()
            {
                continue;
            }

            let account_id = entry.file_name().to_string_lossy().to_string();
            let codex_home = entry.path().join("codex-home");
            if !auth_file_ready(&codex_home)? {
                continue;
            }

            if let Some(account) = store
                .accounts
                .iter_mut()
                .find(|account| account.id == account_id)
            {
                if hydrate_account_metadata(account)? {
                    changed = true;
                }
                continue;
            }

            let mut account = load_account_from_codex_home(account_id, codex_home)?;
            let _ = hydrate_account_metadata(&mut account)?;
            store.accounts.insert(0, account);
            changed = true;
        }

        if store.active_account_id.as_ref().is_some_and(|active_id| {
            store
                .accounts
                .iter()
                .all(|account| &account.id != active_id)
        }) {
            store.active_account_id = None;
            changed = true;
        }

        if store.active_account_id.is_none() && !store.accounts.is_empty() {
            store.active_account_id = store.accounts.first().map(|account| account.id.clone());
            changed = true;
        }

        Ok(changed)
    }
}

fn hydrate_account_metadata(account: &mut StoredAccount) -> Result<bool, String> {
    let auth = read_auth_file(Path::new(&account.codex_home))?;
    let next_kind = match auth.auth_mode.as_deref() {
        Some("chatgpt") => AccountKind::Chatgpt,
        _ => AccountKind::ApiKey,
    };
    let next_metadata = auth_chatgpt_metadata(&auth).unwrap_or((None, None, None));
    let next_label = default_label_for_auth(&auth);
    let should_refresh_label = account.label.trim().is_empty()
        || matches!(account.label.as_str(), "ChatGPT Account" | "OpenAI API Key");
    let changed = account.kind != next_kind
        || (should_refresh_label && account.label != next_label)
        || account.email != next_metadata.0
        || account.plan_type != next_metadata.1
        || account.subscription_active_until != next_metadata.2;

    account.kind = next_kind;
    if should_refresh_label {
        account.label = next_label;
    }
    account.email = next_metadata.0;
    account.plan_type = next_metadata.1;
    account.subscription_active_until = next_metadata.2;
    Ok(changed)
}

fn default_label_for_auth(auth: &AuthFile) -> String {
    match auth.auth_mode.as_deref() {
        Some("chatgpt") => auth_chatgpt_metadata(auth)
            .and_then(|(email, _, _)| email)
            .unwrap_or_else(|| "ChatGPT Account".to_string()),
        _ => auth
            .openai_api_key
            .clone()
            .map(mask_secret)
            .unwrap_or_else(|| "OpenAI API Key".to_string()),
    }
}

fn read_auth_file(codex_home: &Path) -> Result<AuthFile, String> {
    let auth_path = codex_home.join("auth.json");
    let raw = fs::read_to_string(&auth_path).map_err(|error| {
        format!(
            "failed to read Codex auth file {}: {error}",
            auth_path.display()
        )
    })?;
    serde_json::from_str(&raw).map_err(|error| format!("failed to parse Codex auth file: {error}"))
}

fn auth_file_ready(codex_home: &Path) -> Result<bool, String> {
    let auth_path = codex_home.join("auth.json");
    if !auth_path.exists() {
        return Ok(false);
    }

    let auth = read_auth_file(codex_home)?;
    Ok(auth_bearer(&auth).is_some())
}

fn auth_file_usable(codex_home: &Path) -> bool {
    auth_file_ready(codex_home).unwrap_or(false)
}

fn auth_chatgpt_metadata(
    auth: &AuthFile,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let id_token = auth.tokens.as_ref()?.id_token.as_ref()?;
    let claims = decode_jwt_claims::<IdTokenClaims>(id_token).ok()?;
    let auth_claims = claims.auth;
    Some((
        claims.email,
        auth_claims
            .as_ref()
            .and_then(|claims| claims.chatgpt_plan_type.clone()),
        auth_claims.and_then(|claims| claims.chatgpt_subscription_active_until),
    ))
}

async fn refresh_limits_for_account(account: &mut StoredAccount) -> Result<(), String> {
    let auth = read_auth_file(Path::new(&account.codex_home))?;
    if let Some((email, plan_type, subscription_active_until)) = auth_chatgpt_metadata(&auth) {
        if account.email.is_none() {
            account.email = email;
        }
        account.plan_type = plan_type;
        account.subscription_active_until = subscription_active_until;
    }

    account.rate_limits = fetch_rate_limits(&auth).await?;
    Ok(())
}

async fn fetch_rate_limits(auth: &AuthFile) -> Result<RateLimitSnapshot, String> {
    if auth.auth_mode.as_deref() == Some("chatgpt") {
        return fetch_chatgpt_rate_limits(auth).await;
    }

    fetch_api_key_rate_limits(auth).await
}

async fn fetch_chatgpt_rate_limits(auth: &AuthFile) -> Result<RateLimitSnapshot, String> {
    let bearer = auth
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.access_token.clone())
        .ok_or_else(|| "chatgpt account has no access token".to_string())?;
    let account_id = auth
        .tokens
        .as_ref()
        .and_then(|tokens| tokens.account_id.clone())
        .ok_or_else(|| "chatgpt account has no account id".to_string())?;

    let response = reqwest::Client::new()
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header("ChatGPT-Account-Id", account_id)
        .header(USER_AGENT, "codex-cli")
        .send()
        .await
        .map_err(|error| format!("failed to fetch ChatGPT account limits: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Ok(RateLimitSnapshot {
            checked_at: Some(now_iso()),
            note: Some(format!("ChatGPT usage endpoint responded with {}", status)),
            ..RateLimitSnapshot::default()
        });
    }

    let payload: ChatGptUsagePayload = response
        .json()
        .await
        .map_err(|error| format!("failed to parse ChatGPT account limits: {error}"))?;

    let primary = payload
        .rate_limit
        .as_ref()
        .and_then(|limit| limit.primary_window.as_ref());
    let secondary = payload
        .rate_limit
        .as_ref()
        .and_then(|limit| limit.secondary_window.as_ref());

    let checked_at = now_unix_seconds();

    Ok(RateLimitSnapshot {
        primary_used_percent: primary.map(|window| window.used_percent),
        primary_window_minutes: primary
            .and_then(|window| window_minutes(window.limit_window_seconds)),
        primary_resets_at: primary
            .and_then(|window| usage_window_reset_at(window, checked_at))
            .map(|timestamp| timestamp.to_string()),
        secondary_used_percent: secondary.map(|window| window.used_percent),
        secondary_window_minutes: secondary
            .and_then(|window| window_minutes(window.limit_window_seconds)),
        secondary_resets_at: secondary
            .and_then(|window| usage_window_reset_at(window, checked_at))
            .map(|timestamp| timestamp.to_string()),
        credits_balance: payload
            .credits
            .as_ref()
            .and_then(|credits| credits.balance.clone()),
        credits_unlimited: payload.credits.and_then(|credits| credits.unlimited),
        checked_at: Some(checked_at.to_string()),
        note: Some(
            payload
                .plan_type
                .map(|plan| format!("Live usage fetched from ChatGPT ({plan})"))
                .unwrap_or_else(|| "Live usage fetched from ChatGPT".to_string()),
        ),
        ..RateLimitSnapshot::default()
    })
}

async fn fetch_api_key_rate_limits(auth: &AuthFile) -> Result<RateLimitSnapshot, String> {
    let bearer =
        auth_bearer(auth).ok_or_else(|| "account has no usable Codex credential".to_string())?;
    let response = reqwest::Client::new()
        .get("https://api.openai.com/v1/models")
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .send()
        .await
        .map_err(|error| format!("failed to fetch account limits from OpenAI: {error}"))?;

    let headers = response.headers().clone();
    if !response.status().is_success() {
        return Ok(RateLimitSnapshot {
            checked_at: Some(now_iso()),
            note: Some(format!(
                "OpenAI responded with {} while fetching limits",
                response.status()
            )),
            ..RateLimitSnapshot::default()
        });
    }

    Ok(RateLimitSnapshot {
        requests_limit: header_string(&headers, "x-ratelimit-limit-requests"),
        requests_remaining: header_string(&headers, "x-ratelimit-remaining-requests"),
        requests_reset: header_string(&headers, "x-ratelimit-reset-requests"),
        tokens_limit: header_string(&headers, "x-ratelimit-limit-tokens"),
        tokens_remaining: header_string(&headers, "x-ratelimit-remaining-tokens"),
        tokens_reset: header_string(&headers, "x-ratelimit-reset-tokens"),
        checked_at: Some(now_iso()),
        note: Some("Rate limits fetched from OpenAI response headers".to_string()),
        ..RateLimitSnapshot::default()
    })
}

fn load_account_from_codex_home(
    account_id: String,
    codex_home: PathBuf,
) -> Result<StoredAccount, String> {
    let auth = read_auth_file(&codex_home)?;
    let kind = match auth.auth_mode.as_deref() {
        Some("chatgpt") => AccountKind::Chatgpt,
        _ => AccountKind::ApiKey,
    };
    let (email, plan_type, subscription_active_until) =
        auth_chatgpt_metadata(&auth).unwrap_or((None, None, None));

    Ok(StoredAccount {
        id: account_id,
        kind,
        label: default_label_for_auth(&auth),
        codex_home: codex_home.to_string_lossy().to_string(),
        email,
        plan_type,
        subscription_active_until,
        rate_limits: RateLimitSnapshot::default(),
    })
}

fn payload_from_store(store: &AccountsStore) -> AccountsPayload {
    AccountsPayload {
        active_account_id: store.active_account_id.clone(),
        accounts: store.accounts.iter().map(account_summary).collect(),
    }
}

fn account_summary(account: &StoredAccount) -> AccountSummary {
    let masked_secret = if account.kind == AccountKind::ApiKey {
        read_auth_file(Path::new(&account.codex_home))
            .ok()
            .and_then(|auth| auth.openai_api_key)
            .map(mask_secret)
    } else {
        None
    };

    AccountSummary {
        id: account.id.clone(),
        kind: account.kind.clone(),
        label: account.label.clone(),
        email: account.email.clone(),
        plan_type: account.plan_type.clone(),
        subscription_active_until: account.subscription_active_until.clone(),
        masked_secret,
        rate_limits: account.rate_limits.clone(),
    }
}

fn mask_secret(secret: String) -> String {
    let secret_char_len = secret.chars().count();
    if secret_char_len <= 8 {
        return "*".repeat(secret_char_len);
    }
    let prefix = secret.chars().take(3).collect::<String>();
    let suffix = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{}{}{}",
        prefix,
        "*".repeat(secret_char_len.saturating_sub(7).max(4)),
        suffix
    )
}

fn auth_bearer(auth: &AuthFile) -> Option<String> {
    match auth.auth_mode.as_deref() {
        Some("chatgpt") => auth.tokens.as_ref()?.access_token.clone(),
        _ => auth.openai_api_key.clone(),
    }
}

fn header_string(headers: &reqwest::header::HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn decode_jwt_claims<T: for<'de> Deserialize<'de>>(token: &str) -> Result<T, String> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| "malformed JWT payload".to_string())?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| format!("failed to decode JWT payload: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("failed to parse JWT payload: {error}"))
}

fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            while let Some(next) = chars.next() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        output.push(ch);
    }
    output
}

fn is_device_code(part: &str) -> bool {
    let mut pieces = part.split('-');
    matches!((pieces.next(), pieces.next(), pieces.next()), (Some(left), Some(right), None) if left.len() == 4 && right.len() == 5 && left.chars().all(|ch| ch.is_ascii_alphanumeric()) && right.chars().all(|ch| ch.is_ascii_alphanumeric()))
}

fn window_minutes(seconds: Option<i64>) -> Option<i64> {
    let seconds = seconds?;
    Some((seconds / 60).max(1))
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("acct-{nanos:x}")
}

fn now_iso() -> String {
    let seconds = now_unix_seconds();
    format!("{seconds}")
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn usage_window_reset_at(window: &ChatGptUsageWindow, checked_at: i64) -> Option<i64> {
    window.reset_at.or_else(|| {
        window
            .reset_after_seconds
            .map(|seconds| checked_at.saturating_add(seconds.max(0)))
    })
}

fn account_runtime_score(account: &StoredAccount) -> Result<Option<i64>, String> {
    if account_unavailable_reason(account)?.is_some() {
        return Ok(None);
    }

    let score = match account.kind {
        AccountKind::ApiKey => 10_000,
        AccountKind::Chatgpt => {
            let primary = account.rate_limits.primary_used_percent.unwrap_or(0.0);
            let secondary = account.rate_limits.secondary_used_percent.unwrap_or(0.0);
            (10_000.0 - (primary.max(secondary) * 100.0)).round() as i64
        }
    };

    Ok(Some(score))
}

fn account_unavailable_reason(account: &StoredAccount) -> Result<Option<String>, String> {
    let codex_home = Path::new(&account.codex_home);
    if !auth_file_ready(codex_home)? {
        return Ok(Some("missing usable auth".into()));
    }

    if account.kind == AccountKind::Chatgpt {
        if account
            .rate_limits
            .secondary_used_percent
            .is_some_and(|value| value >= 100.0)
        {
            return Ok(Some("weekly usage limit reached".into()));
        }
        if account
            .rate_limits
            .primary_used_percent
            .is_some_and(|value| value >= 100.0)
        {
            return Ok(Some("window usage limit reached".into()));
        }
        return Ok(None);
    }

    if account.rate_limits.requests_remaining.as_deref() == Some("0") {
        return Ok(Some("request rate limit reached".into()));
    }
    if account.rate_limits.tokens_remaining.as_deref() == Some("0") {
        return Ok(Some("token rate limit reached".into()));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("miawk-accounts-tests-{label}-{}", new_id()));
            fs::create_dir_all(&path).expect("create test data dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_auth_file(codex_home: &Path) {
        fs::create_dir_all(codex_home).expect("create codex home");
        fs::write(
            codex_home.join("auth.json"),
            r#"{"OPENAI_API_KEY":"sk-test-key"}"#,
        )
        .expect("write auth file");
    }

    fn write_unusable_auth_file(codex_home: &Path) {
        fs::create_dir_all(codex_home).expect("create codex home");
        fs::write(codex_home.join("auth.json"), "{}").expect("write unusable auth file");
    }

    fn stored_account(account_id: &str, codex_home: &Path) -> StoredAccount {
        StoredAccount {
            id: account_id.to_string(),
            kind: AccountKind::ApiKey,
            label: "Test account".to_string(),
            codex_home: codex_home.to_string_lossy().into_owned(),
            email: None,
            plan_type: None,
            subscription_active_until: None,
            rate_limits: RateLimitSnapshot::default(),
        }
    }

    #[test]
    fn migrate_store_account_paths_switches_to_current_after_successful_move() {
        let data_dir = TestDir::new("switch-after-move");
        let service = AccountsService::new(data_dir.path().to_path_buf());
        let account_id = "acct-switch";
        let legacy_home = data_dir
            .path()
            .join("legacy")
            .join(account_id)
            .join("codex-home");
        write_auth_file(&legacy_home);

        let mut store = AccountsStore {
            active_account_id: Some(account_id.to_string()),
            accounts: vec![stored_account(account_id, &legacy_home)],
        };

        let changed = service.migrate_store_account_paths(&mut store);
        let current_home = service.account_codex_home(account_id);

        assert!(changed);
        assert_eq!(store.accounts[0].codex_home, current_home.to_string_lossy());
        assert!(current_home.join("auth.json").exists());
    }

    #[test]
    fn migrate_store_account_paths_keeps_working_home_when_move_fails() {
        let data_dir = TestDir::new("keep-on-failure");
        let service = AccountsService::new(data_dir.path().to_path_buf());
        let account_id = "acct-keep";
        let legacy_home = data_dir
            .path()
            .join("legacy")
            .join(account_id)
            .join("codex-home");
        write_auth_file(&legacy_home);

        let current_home = service.account_codex_home(account_id);
        fs::create_dir_all(&current_home).expect("create blocking current home");
        fs::write(current_home.join("blocking.txt"), "not empty").expect("write blocking file");

        let mut store = AccountsStore {
            active_account_id: Some(account_id.to_string()),
            accounts: vec![stored_account(account_id, &legacy_home)],
        };

        let changed = service.migrate_store_account_paths(&mut store);

        assert!(!changed);
        assert_eq!(store.accounts[0].codex_home, legacy_home.to_string_lossy());
        assert!(legacy_home.join("auth.json").exists());
        assert!(!current_home.join("auth.json").exists());
    }

    #[test]
    fn migrate_store_account_paths_prefers_current_when_auth_already_exists() {
        let data_dir = TestDir::new("prefer-current");
        let service = AccountsService::new(data_dir.path().to_path_buf());
        let account_id = "acct-current";
        let legacy_home = data_dir
            .path()
            .join("legacy")
            .join(account_id)
            .join("codex-home");
        write_auth_file(&legacy_home);

        let current_home = service.account_codex_home(account_id);
        write_auth_file(&current_home);

        let mut store = AccountsStore {
            active_account_id: Some(account_id.to_string()),
            accounts: vec![stored_account(account_id, &legacy_home)],
        };

        let changed = service.migrate_store_account_paths(&mut store);

        assert!(changed);
        assert_eq!(store.accounts[0].codex_home, current_home.to_string_lossy());
    }

    #[test]
    fn migrate_store_account_paths_uses_legacy_when_current_auth_is_unusable() {
        let root_dir = TestDir::new("skip-unusable-stored-home");
        let data_dir = root_dir.path().join("com.melani.miawk");
        fs::create_dir_all(&data_dir).expect("create current data dir");
        let service = AccountsService::new(data_dir.clone());
        let account_id = "acct-unusable";

        let current_home = service.account_codex_home(account_id);
        write_unusable_auth_file(&current_home);

        let legacy_home = data_dir
            .parent()
            .expect("data dir has parent")
            .join("com.melani.rsc")
            .join("accounts")
            .join(account_id)
            .join("codex-home");
        write_auth_file(&legacy_home);

        let mut store = AccountsStore {
            active_account_id: Some(account_id.to_string()),
            accounts: vec![stored_account(account_id, &current_home)],
        };

        let changed = service.migrate_store_account_paths(&mut store);

        assert!(changed);
        assert_eq!(store.accounts[0].codex_home, legacy_home.to_string_lossy());
    }

    #[test]
    fn read_store_persists_migrated_account_paths() {
        let data_dir = TestDir::new("persist-migrated-paths");
        let service = AccountsService::new(data_dir.path().to_path_buf());
        let account_id = "acct-persist";
        let legacy_home = data_dir
            .path()
            .join("legacy")
            .join(account_id)
            .join("codex-home");
        write_auth_file(&legacy_home);

        let initial = AccountsStore {
            active_account_id: Some(account_id.to_string()),
            accounts: vec![stored_account(account_id, &legacy_home)],
        };
        service.write_store(&initial).expect("write initial store");

        let migrated = service.read_store().expect("read and migrate store");
        let current_home = service.account_codex_home(account_id);

        assert_eq!(
            migrated.accounts[0].codex_home,
            current_home.to_string_lossy()
        );

        let persisted_raw =
            fs::read_to_string(service.store_path()).expect("read migrated store from disk");
        let persisted: AccountsStore =
            serde_json::from_str(&persisted_raw).expect("parse migrated store");

        assert_eq!(
            persisted.accounts[0].codex_home,
            current_home.to_string_lossy()
        );
    }
}
