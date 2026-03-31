//! IPC command handlers for the Tauri frontend.

use serde::Serialize;
use tauri::State;
use tracing::instrument;

use crate::monitor::{self, CaptureInfo, MonitorConfig, MonitorState, MonitorStatus};

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

/// Update monitor configuration and persist to disk.
///
/// # Errors
///
/// Returns an error if the config lock is poisoned.
#[tauri::command]
#[allow(clippy::unreachable, clippy::needless_pass_by_value)]
pub async fn save_settings(
    app_handle: tauri::AppHandle,
    state: State<'_, MonitorState>,
    config: MonitorConfig,
) -> Result<(), String> {
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

    Ok(())
}
