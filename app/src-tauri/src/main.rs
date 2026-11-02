// No console window behind the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod commands;
mod device;
mod error;
mod jobs;
mod selftest;

use tauri::Manager;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--self-test") {
        std::process::exit(selftest::run());
    }
    let simulated = args.iter().any(|a| a == "--sim");
    applog::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // a second launch just brings the open window forward
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(device::Device::default())
        .manage(jobs::Jobs::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            applog::attach(handle.clone());
            log::info!(
                "CalcBridge {} started{}",
                env!("CARGO_PKG_VERSION"),
                if simulated { " with the simulator" } else { "" }
            );
            jobs::start(handle.clone());
            device::start(handle, simulated);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::connect,
            commands::disconnect,
            commands::refresh_info,
            commands::set_auto_connect,
            commands::usb_devices,
            commands::log_lines,
            commands::list_dir,
            commands::exists,
            commands::make_dir,
            commands::rename,
            commands::delete,
            commands::copy_or_move,
            commands::upload,
            commands::download,
            commands::backup,
            commands::restore,
            commands::os_image_check,
            commands::install_os,
            commands::cancel_job,
            commands::screenshot,
            commands::save_screenshot,
            commands::save_png,
            commands::press_key,
            commands::type_text,
            commands::local_list,
            commands::local_places,
            commands::local_make_dir,
            commands::local_rename,
            commands::local_exists,
            commands::local_is_dir,
            commands::write_text_file,
            commands::read_text_file,
            commands::preview_notes,
            commands::save_document,
            commands::send_document,
            commands::install_module,
            commands::open_local_document,
            commands::open_handheld_document,
            commands::save_code_files,
            commands::local_name_for,
        ])
        .run(tauri::generate_context!())
        .expect("CalcBridge couldn't start");
}
