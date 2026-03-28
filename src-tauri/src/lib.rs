//! Tauri v2 application setup and IPC command registration.

mod telemetry;

/// Greet command for initial connectivity verification.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! Welcome to DNA Assistant.")
}

/// Run the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to initialize.
#[allow(clippy::missing_errors_doc, clippy::expect_used, clippy::exit)]
pub fn run() {
    let _guard = telemetry::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("failed to run tauri application");
}
