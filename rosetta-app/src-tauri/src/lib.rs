mod managed_rwkv;
mod onboarding;
mod rosetta_jobs;
mod rwkv_api;
mod rwkv_providers;
#[allow(dead_code)]
mod rwkv_runtime;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(rwkv_api::RwkvTranslationRunRegistry::default())
        .manage(managed_rwkv::Registry::default())
        .manage(managed_rwkv::InstallStateRegistry::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Both `main` and `onboarding` are declared in tauri.conf.json
            // with `visible: false`. Pick one to show now based on whether
            // the user has completed onboarding AND the model is on disk.
            // Bundled `.app` launched from Finder lands here too — this is
            // the only entry point that decides "fresh user vs returning
            // user".
            let handle = app.handle();
            let decision = onboarding::decide(handle);
            let target_label = if decision.needs_onboarding {
                "onboarding"
            } else {
                "main"
            };
            if let Some(window) = handle.get_webview_window(target_label) {
                window.show().ok();
                window.set_focus().ok();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            managed_rwkv::cancel_managed_rwkv_install,
            managed_rwkv::get_managed_rwkv_install_plan,
            managed_rwkv::get_managed_rwkv_install_progress,
            managed_rwkv::get_managed_rwkv_runtime_logs_summary,
            managed_rwkv::get_managed_rwkv_runtime_status,
            managed_rwkv::install_managed_rwkv_runtime,
            managed_rwkv::probe_managed_rwkv_runtime,
            managed_rwkv::start_managed_rwkv_runtime,
            managed_rwkv::stop_managed_rwkv_runtime,
            onboarding::complete_onboarding_and_open_main,
            onboarding::get_onboarding_decision,
            onboarding::reopen_onboarding_window,
            rosetta_jobs::create_rosetta_translation_revision,
            rosetta_jobs::delete_rosetta_job,
            rosetta_jobs::delete_rosetta_job_file,
            rosetta_jobs::ensure_rosetta_translation_file,
            rosetta_jobs::export_rosetta_job_file,
            rosetta_jobs::export_rosetta_translation_file,
            rosetta_jobs::import_rosetta_document_from_path,
            rosetta_jobs::import_rosetta_project_from_directory,
            rosetta_jobs::list_rosetta_jobs,
            rosetta_jobs::load_rosetta_job,
            rosetta_jobs::load_rosetta_translation_file,
            rosetta_jobs::pick_rosetta_export_path,
            rosetta_jobs::pick_rosetta_import_directory,
            rosetta_jobs::pick_rosetta_import_path,
            rosetta_jobs::rename_rosetta_job,
            rosetta_jobs::save_rosetta_segments,
            rosetta_jobs::save_rosetta_translation_segments,
            rosetta_jobs::update_rosetta_job_file_languages,
            rwkv_api::cancel_rwkv_translation_run,
            rwkv_api::get_rwkv_translation_run_status,
            rwkv_api::probe_rwkv_mobile_batch_chat,
            rwkv_api::probe_rwkv_translation_api,
            rwkv_api::start_rwkv_mobile_batch_chat_run,
            rwkv_api::start_rwkv_translation_run,
            rwkv_api::translate_rwkv_mobile_batch_chat_texts,
            rwkv_api::translate_rwkv_texts_with_api
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
