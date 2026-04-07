//! IPC command handlers for the Tauri frontend.

use serde::Serialize;
use tauri::State;
use tracing::instrument;
use tracing_subscriber::EnvFilter;

use crate::monitor::{self, CaptureInfo, MonitorConfig, MonitorState, MonitorStatus};
use crate::notification::NotificationManager;
use crate::telemetry::EnvFilterHandle;

/// Start the background monitoring loop.
///
/// # Errors
///
/// Returns an error string if monitoring is already active or cannot be started.
#[tauri::command]
#[instrument(skip_all)]
#[allow(clippy::unreachable, clippy::needless_pass_by_value)]
pub async fn start_monitoring(
    app_handle: tauri::AppHandle,
    state: State<'_, MonitorState>,
) -> Result<(), String> {
    monitor::start(app_handle, &state).map_err(|e| format!("{e:#}"))
}

/// Stop the background monitoring loop.
#[tauri::command]
#[instrument(skip_all)]
#[allow(clippy::unreachable, clippy::needless_pass_by_value)]
pub async fn stop_monitoring(
    app_handle: tauri::AppHandle,
    state: State<'_, MonitorState>,
) -> Result<(), String> {
    monitor::stop(&app_handle, &state);
    Ok(())
}

/// Get the current monitoring status.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_status(state: State<'_, MonitorState>) -> MonitorStatus {
    state
        .status
        .lock()
        .map_or_else(|_| MonitorStatus::default(), |s| s.clone())
}

/// Response for the capture preview IPC command.
#[derive(Debug, Clone, Serialize)]
pub struct CapturePreview {
    /// Base64-encoded PNG image data.
    pub image_base64: Option<String>,
    /// Capture metadata.
    pub info: CaptureInfo,
}

/// Maximum preview width for the Detection page (smaller = faster encode).
const PREVIEW_MAX_WIDTH: u32 = 640;

/// Get the latest captured frame as base64 PNG + metadata.
///
/// The raw frame is downscaled and PNG-encoded on demand (not in the
/// capture loop), so this call takes ~10-50ms depending on frame size.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_capture_preview(state: State<'_, MonitorState>) -> CapturePreview {
    use std::io::Cursor;

    use base64::Engine;
    use image::ImageFormat;
    use image::imageops::FilterType;

    // Clone Arc + info under the lock, then drop it before encoding
    let (image_arc, info) = {
        let Ok(guard) = state.latest_frame.lock() else {
            return CapturePreview {
                image_base64: None,
                info: CaptureInfo::default(),
            };
        };
        (guard.image.clone(), guard.info.clone())
    };

    let image_base64 = image_arc.as_ref().and_then(|img| {
        let preview = if img.width() > PREVIEW_MAX_WIDTH {
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::as_conversions
            )]
            let h = (f64::from(img.height()) * f64::from(PREVIEW_MAX_WIDTH)
                / f64::from(img.width())) as u32;
            image::imageops::resize(img.as_ref(), PREVIEW_MAX_WIDTH, h, FilterType::Triangle)
        } else {
            img.as_ref().clone()
        };

        let mut buf = Cursor::new(Vec::new());
        preview.write_to(&mut buf, ImageFormat::Png).ok()?;
        Some(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
    });

    CapturePreview { image_base64, info }
}

/// Get current monitor configuration.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_settings(state: State<'_, MonitorState>) -> MonitorConfig {
    state
        .config
        .lock()
        .map_or_else(|_| MonitorConfig::default(), |c| c.clone())
}

/// Get default monitor configuration (single source of truth for Reset).
#[tauri::command]
pub fn get_default_settings() -> MonitorConfig {
    MonitorConfig::default()
}

/// Response for [`save_settings`].
#[derive(Debug, Serialize)]
pub struct SaveSettingsResult {
    /// Whether an app restart is required to apply the OTel configuration change.
    pub restart_required: bool,
}

/// Update monitor configuration and persist to disk.
///
/// - `debug_rust_log` changes are applied immediately via the reload handle.
/// - `debug_otel_endpoint` / `debug_otel_headers` changes set `restart_required: true`.
///
/// # Errors
///
/// Returns an error if the config lock is poisoned.
#[tauri::command]
#[allow(clippy::unreachable, clippy::needless_pass_by_value)]
pub async fn save_settings(
    app_handle: tauri::AppHandle,
    state: State<'_, MonitorState>,
    filter_handle: State<'_, EnvFilterHandle>,
    config: MonitorConfig,
) -> Result<SaveSettingsResult, String> {
    // Snapshot only the debug fields we need to compare (avoid holding the lock).
    let (old_rust_log, old_otel_endpoint, old_otel_headers) = {
        let guard = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        (
            guard.debug_rust_log.clone(),
            guard.debug_otel_endpoint.clone(),
            guard.debug_otel_headers.clone(),
        )
    };

    // Hot-reload RUST_LOG if the directive changed.
    if config.debug_rust_log != old_rust_log {
        let directive = if config.debug_rust_log.is_empty() {
            std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,dna=info".to_owned())
        } else {
            config.debug_rust_log.clone()
        };
        match EnvFilter::try_new(&directive) {
            Ok(new_filter) => {
                if let Err(e) = filter_handle.reload(new_filter) {
                    tracing::warn!("Failed to reload log filter: {e}");
                }
            }
            Err(e) => tracing::warn!("Invalid RUST_LOG directive {directive:?}: {e}"),
        }
    }

    // OTel fields require a restart to apply (providers are built once at startup).
    let restart_required = config.debug_otel_endpoint != old_otel_endpoint
        || config.debug_otel_headers != old_otel_headers;

    {
        let mut guard = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        *guard = config.clone();
    }
    crate::settings::save(&app_handle, &config);

    // If monitoring is active, restart to apply new config
    let is_active = state.handle.lock().map(|h| h.is_some()).unwrap_or(false);
    if is_active {
        monitor::stop(&app_handle, &state);
        monitor::start(app_handle, &state).map_err(|e| format!("{e:#}"))?;
    }

    Ok(SaveSettingsResult { restart_required })
}

/// Restart the application to apply configuration changes that cannot be hot-reloaded.
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// Send a test notification (Discord or Windows toast depending on config).
#[tauri::command]
#[instrument(skip_all)]
#[allow(clippy::needless_pass_by_value)]
pub fn test_notification(state: State<'_, MonitorState>) {
    let config = state
        .config
        .lock()
        .map_or_else(|_| MonitorConfig::default(), |c| c.clone());
    NotificationManager::send_test_notification(&config);
}
