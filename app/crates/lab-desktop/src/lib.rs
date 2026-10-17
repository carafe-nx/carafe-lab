//! Carafe Lab's window: the composition root of the Tauri shell over the lab's crates and `carafe-lab`.

mod commands;
mod detached;
mod error;
mod lab;
mod novnc;

use std::sync::Arc;

use tauri::Manager;

use crate::lab::Lab;

/// Starts the window; if Tauri fails to start it, prints the error and exits the process with code 1.
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Lab::new()))
        .register_asynchronous_uri_scheme_protocol(novnc::SCHEME, |context, request, responder| {
            let lab = Arc::clone(context.app_handle().state::<Arc<Lab>>().inner());
            let path = request.uri().path().to_owned();
            std::thread::spawn(move || responder.respond(novnc::respond(&lab, &path)));
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_lab,
            commands::list_runs,
            commands::version_steps,
            commands::run_events,
            commands::start_run,
            commands::stop_run,
            commands::add_game,
            commands::run_screenshots,
            commands::game_problems,
            commands::save_scenario,
            commands::save_game_notes,
            commands::choose_keys,
            commands::forget_keys,
            commands::choose_firmware,
            commands::read_nsp,
            commands::record_console_verdict,
            commands::forget_firmware,
            commands::open_path,
            commands::read_screen,
            commands::live_perf,
            commands::read_image,
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Carafe Lab failed to start: {error}");
        std::process::exit(1);
    }
}
