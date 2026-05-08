mod rwkv_api;
mod rwkv_runtime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            rwkv_api::probe_rwkv_translation_api,
            rwkv_api::translate_rwkv_texts_with_api,
            rwkv_runtime::get_rwkv_runtime_artifact_catalog,
            rwkv_runtime::get_rwkv_runtime_install_progress,
            rwkv_runtime::get_rwkv_runtime_install_plan,
            rwkv_runtime::get_rwkv_runtime_process_status,
            rwkv_runtime::get_rwkv_runtime_status,
            rwkv_runtime::initialize_rwkv_runtime_layout,
            rwkv_runtime::probe_rwkv_runtime_translation,
            rwkv_runtime::prepare_rwkv_runtime_install,
            rwkv_runtime::extract_rwkv_runtime_artifact,
            rwkv_runtime::scan_rwkv_runtime_artifacts,
            rwkv_runtime::start_rwkv_runtime
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
