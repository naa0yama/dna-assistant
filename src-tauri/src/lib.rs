//! Tauri v2 application setup and IPC command registration.

#[cfg(target_os = "windows")]
#[allow(clippy::unreachable)] // Tauri command macro generates unreachable in Result paths
mod commands;
mod monitor;
#[cfg(target_os = "windows")]
mod notification;
#[cfg(target_os = "windows")]
mod settings;
mod telemetry;

use monitor::MonitorState;

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
fn build(filter_handle: telemetry::EnvFilterHandle) -> tauri::Result<tauri::App> {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .manage(MonitorState::new())
        .manage(filter_handle);

    #[cfg(target_os = "windows")]
    {
        use tauri::Manager;
        builder = builder.setup(|app| {
            let state = app.state::<MonitorState>();
            state.load_config(app.handle());
            Ok(())
        });
        builder = builder.invoke_handler(tauri::generate_handler![
            greet,
            commands::start_monitoring,
            commands::stop_monitoring,
            commands::get_status,
            commands::get_capture_preview,
            commands::get_settings,
            commands::get_default_settings,
            commands::save_settings,
            commands::test_notification,
            commands::restart_app,
        ]);
    }

    #[cfg(not(target_os = "windows"))]
    {
        builder = builder.invoke_handler(tauri::generate_handler![greet,]);
    }

    builder.build(tauri::generate_context!())
}

/// Run the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to initialize.
#[allow(clippy::missing_errors_doc, clippy::expect_used, clippy::exit)]
pub fn run() {
    // Inject debug overrides as process-local env vars before telemetry init.
    // Using set_var here is safe: this runs single-threaded before any threads are spawned.
    #[cfg(target_os = "windows")]
    {
        let pre_config = settings::pre_load();
        if !pre_config.debug_rust_log.is_empty() {
            std::env::set_var("RUST_LOG", &pre_config.debug_rust_log);
        }
        if !pre_config.debug_otel_endpoint.is_empty() {
            std::env::set_var(
                "OTEL_EXPORTER_OTLP_ENDPOINT",
                &pre_config.debug_otel_endpoint,
            );
        }
        if !pre_config.debug_otel_headers.is_empty() {
            std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", &pre_config.debug_otel_headers);
        }
    }

    let (_guard, filter_handle) = telemetry::init();

    // Install app-level metrics instruments when OTel is enabled.
    // `_guard` is intentionally accessed here; the underscore keeps it alive on non-Windows.
    #[cfg(all(target_os = "windows", feature = "otel"))]
    #[allow(clippy::used_underscore_binding)]
    if let Some(meter) = _guard.meter() {
        telemetry::metrics::install(&meter);
    }

    build(filter_handle)
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
