//! DocShift - convert, compress and edit PDFs, Office documents, images and video.
mod cli;
mod commands;
mod core;
mod engine;
mod imgops;
mod jobs;
mod kinds;
mod ocr;
mod office;
mod pdf2x;
mod pdfa;
mod pdfops;
mod render;
mod textout;
mod video;

/// The window opens at 1280 x 820, or smaller on small screens (laptops at 1366 x 768 with the taskbar), centred
/// in the area the taskbar leaves free. It starts hidden so this happens before anything is drawn.
fn fit_to_screen(w: &tauri::WebviewWindow) {
    let Ok(Some(m)) = w.current_monitor().or_else(|_| w.primary_monitor()) else { return };
    let scale = m.scale_factor();
    let area = m.work_area();
    let (aw, ah) = (area.size.width as f64 / scale, area.size.height as f64 / scale);
    let (width, height) = (1280f64.min(aw - 16.0), 820f64.min(ah - 16.0));
    let _ = w.set_size(tauri::LogicalSize::new(width, height));
    let x = area.position.x as f64 / scale + (aw - width) / 2.0;
    let y = area.position.y as f64 / scale + (ah - height) / 2.0;
    let _ = w.set_position(tauri::LogicalPosition::new(x.max(0.0), y.max(0.0)));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Some(code) = cli::maybe_run() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(jobs::Registry::default())
        .setup(|app| {
            engine::warm_up();
            if let Some(w) = tauri::Manager::get_webview_window(app, "main") {
                fit_to_screen(&w);
                let _ = w.show();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::inspect_files,
            commands::thumbnail,
            commands::pdf_info,
            commands::engine_status,
            commands::default_output_dir,
            commands::check_output_dir,
            commands::validate_target,
            commands::validate_pages,
            commands::start_job,
            commands::cancel_job,
            commands::ocr_languages,
            commands::open_path,
            commands::answer_ask,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DocShift");
}
