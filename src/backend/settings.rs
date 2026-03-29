use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "settings.json";

fn default_auto_drive_mode() -> String {
    "completion".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppSettings {
    #[serde(default = "default_agent_max_threads")]
    pub agent_max_threads: usize,
    #[serde(default = "default_agent_max_depth")]
    pub agent_max_depth: i32,
    #[serde(default)]
    pub auto_drive_enabled: bool,
    #[serde(default = "default_auto_drive_mode")]
    pub auto_drive_mode: String,
    #[serde(default)]
    pub auto_drive_max_turns: Option<u32>,
    #[serde(default)]
    pub auto_drive_max_runtime_hours: Option<u32>,
    #[serde(default = "default_workspace_path_string")]
    pub current_workspace_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            agent_max_threads: default_agent_max_threads(),
            agent_max_depth: default_agent_max_depth(),
            auto_drive_enabled: false,
            auto_drive_mode: default_auto_drive_mode(),
            auto_drive_max_turns: None,
            auto_drive_max_runtime_hours: None,
            current_workspace_path: default_workspace_path_string(),
        }
    }
}

#[derive(Clone)]
pub struct SettingsService {
    data_dir: PathBuf,
}

impl SettingsService {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn load(&self) -> Result<AppSettings, String> {
        let path = self.path();
        if !path.exists() {
            let settings = AppSettings::default();
            self.ensure_workspace_dir(&settings.current_workspace_path)?;
            return Ok(settings);
        }

        let content = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read settings {}: {error}", path.display()))?;
        let settings: AppSettings = serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse settings {}: {error}", path.display()))?;
        self.ensure_workspace_dir(&settings.current_workspace_path)?;
        Ok(settings)
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), String> {
        let path = self.path();
        self.ensure_workspace_dir(&settings.current_workspace_path)?;
        let content = serde_json::to_string_pretty(settings)
            .map_err(|error| format!("failed to serialize settings: {error}"))?;
        fs::write(&path, content)
            .map_err(|error| format!("failed to write settings {}: {error}", path.display()))
    }

    pub fn current_workspace_path(&self) -> Result<PathBuf, String> {
        let settings = self.load()?;
        Ok(PathBuf::from(settings.current_workspace_path))
    }

    fn path(&self) -> PathBuf {
        self.data_dir.join(SETTINGS_FILE)
    }

    fn ensure_workspace_dir(&self, workspace_path: &str) -> Result<(), String> {
        let path = PathBuf::from(workspace_path);
        fs::create_dir_all(&path).map_err(|error| {
            format!(
                "failed to create workspace directory {}: {error}",
                path.display()
            )
        })
    }
}

fn default_agent_max_threads() -> usize {
    16
}

fn default_agent_max_depth() -> i32 {
    1
}

pub fn default_workspace_path_string() -> String {
    default_workspace_path().to_string_lossy().into_owned()
}

pub fn default_workspace_path() -> PathBuf {
    if let Some(home_dir) = dirs::home_dir() {
        return home_dir.join("MIAWK").join("General");
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("MIAWK")
        .join("General")
}
