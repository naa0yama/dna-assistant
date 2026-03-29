//! Tauri v2 application setup and IPC command registration.

#[allow(clippy::unreachable)] // Tauri command macro generates unreachable in Result paths
mod commands;
mod monitor;
mod notification;
mod settings;
mod telemetry;

use monitor::MonitorState;
use tauri::Manager;

/// Greet command for initial connectivity verification.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! Welcome to DNA Assistant.")
}

/// Build the Tauri application without starting the event loop.
///
/// # Errors
///
/// Returns an error if the Tauri runtime fails to initialize.
#[allow(clippy::missing_errors_doc, clippy::exit)]
fn build() -> tauri::Result<tauri::App> {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .manage(MonitorState::new())
        .setup(|app| {
            // Load persisted settings from disk
            let state = app.state::<MonitorState>();
            state.load_config(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::start_monitoring,
            commands::stop_monitoring,
            commands::get_status,
            commands::get_capture_preview,
            commands::get_settings,
            commands::get_default_settings,
            commands::save_settings,
        ])
        .build(tauri::generate_context!())
}

/// Run the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to initialize.
#[allow(clippy::missing_errors_doc, clippy::expect_used, clippy::exit)]
pub fn run() {
    let _guard = telemetry::init();

    build()
        .expect("failed to build tauri application")
        .run(|_app, _event| {});
}

#[cfg(test)]
mod tests {
    use tauri::test::{mock_builder, mock_context, noop_assets};

    #[test]
    fn greet_returns_welcome_message() {
        let result = super::greet("Player");
        assert_eq!(result, "Hello, Player! Welcome to DNA Assistant.");
    }

    #[test]
    fn app_builder_succeeds_with_mock_runtime() {
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![super::greet])
            .build(mock_context(noop_assets()));
        assert!(app.is_ok());
    }
}
