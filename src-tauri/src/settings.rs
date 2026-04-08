//! Settings persistence — load/save `MonitorConfig` as JSON in app data dir.

use std::path::PathBuf;

use tracing::{debug, warn};

use crate::monitor::MonitorConfig;

/// Settings filename within the app data directory.
const SETTINGS_FILE: &str = "settings.json";

/// Tauri v2 bundle identifier — must match `tauri.conf.json`.
const APP_IDENTIFIER: &str = "com.naa0yama.dna-assistant";

/// Resolve the settings path without a Tauri `AppHandle`.
///
/// Mirrors Tauri v2's `app_data_dir` resolution on Windows (`%APPDATA%\<id>`).
/// Used at startup before the Tauri runtime is initialised.
fn pre_init_path() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(APP_IDENTIFIER)
        .join(SETTINGS_FILE)
}

/// Load `MonitorConfig` before the Tauri runtime is available.
///
/// Used at startup to read debug field overrides (`debug_rust_log`,
/// `debug_otel_endpoint`, `debug_otel_headers`) and pass them directly to
/// `telemetry::init()` as [`crate::telemetry::TelemetryOverrides`].
/// Returns [`MonitorConfig::default`] if the file is missing or invalid.
pub fn pre_load() -> MonitorConfig {
    let path = pre_init_path();
    let Ok(json) = std::fs::read_to_string(&path) else {
        return MonitorConfig::default();
    };
    serde_json::from_str::<MonitorConfig>(&json).unwrap_or_default()
}

/// Resolve the settings file path.
fn settings_path(app_handle: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;

    app_handle.path().app_data_dir().map_or_else(
        |_| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(SETTINGS_FILE)))
                .unwrap_or_else(|| PathBuf::from(SETTINGS_FILE))
        },
        |d| d.join(SETTINGS_FILE),
    )
}

/// Load `MonitorConfig` from disk, returning default if missing or invalid.
pub fn load(app_handle: &tauri::AppHandle) -> MonitorConfig {
    let path = settings_path(app_handle);
    let Ok(json) = std::fs::read_to_string(&path) else {
        debug!(path = %path.display(), "no settings file, using defaults");
        return MonitorConfig::default();
    };
    match serde_json::from_str::<MonitorConfig>(&json) {
        Ok(config) => {
            debug!(path = %path.display(), "settings loaded");
            config
        }
        Err(e) => {
            warn!(%e, path = %path.display(), "invalid settings file, using defaults");
            MonitorConfig::default()
        }
    }
}

/// Save `MonitorConfig` to disk.
pub fn save(app_handle: &tauri::AppHandle, config: &MonitorConfig) {
    let path = settings_path(app_handle);

    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!(%e, path = %path.display(), "failed to create settings directory");
        return;
    }

    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!(%e, path = %path.display(), "failed to write settings");
            } else {
                debug!(path = %path.display(), "settings saved");
            }
        }
        Err(e) => {
            warn!(%e, "failed to serialize settings");
        }
    }
}
