mod rwkv_runtime;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            rwkv_runtime::get_rwkv_runtime_artifact_catalog,
            rwkv_runtime::get_rwkv_runtime_install_progress,
            rwkv_runtime::get_rwkv_runtime_install_plan,
            rwkv_runtime::get_rwkv_runtime_status,
            rwkv_runtime::initialize_rwkv_runtime_layout,
            rwkv_runtime::prepare_rwkv_runtime_install
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
