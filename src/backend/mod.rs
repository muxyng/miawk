pub mod accounts;
pub mod codex;
pub mod runtime;
pub mod settings;
pub mod swarm;

use std::{path::PathBuf, sync::Arc};

use accounts::AccountsService;
use runtime::AppServerService;
use settings::SettingsService;

#[derive(Clone)]
pub struct AppServices {
    pub accounts: Arc<AccountsService>,
    pub runtime: Arc<AppServerService>,
    pub settings: Arc<SettingsService>,
}

impl AppServices {
    pub fn new() -> Result<Self, String> {
        let data_dir = app_data_dir()?;
        std::fs::create_dir_all(&data_dir).map_err(|error| {
            format!(
                "failed to create app data dir {}: {error}",
                data_dir.display()
            )
        })?;
        Ok(Self {
            accounts: Arc::new(AccountsService::new(data_dir.clone())),
            runtime: Arc::new(AppServerService::new(data_dir.clone())),
            settings: Arc::new(SettingsService::new(data_dir)),
        })
    }
}

pub fn app_data_dir() -> Result<PathBuf, String> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| "failed to resolve local data directory".to_string())?;
    let current = base.join("com.melani.miawk");
    let legacy = base.join("com.melani.rsc");

    if current.exists() || !legacy.exists() {
        return Ok(current);
    }

    match std::fs::rename(&legacy, &current) {
        Ok(()) => Ok(current),
        Err(_) => Ok(legacy),
    }
}
