//! Carafe Lab's window: the composition root of the Tauri shell over the lab's crates and `carafe-lab`.

mod commands;
mod detached;
mod error;
mod lab;

use std::sync::Arc;

use crate::lab::Lab;

/// Starts the window; if Tauri fails to start it, prints the error and exits the process with code 1.
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Lab::new()))
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_lab,
            commands::list_runs,
            commands::run_events,
            commands::start_run,
            commands::stop_run,
            commands::add_game,
            commands::choose_keys,
            commands::forget_keys,
            commands::open_path,
            commands::read_screen,
            commands::read_image,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Carafe Lab failed to start: {error}");
        std::process::exit(1);
    }
}
