pub mod layout;
pub mod openai_shim;
pub mod profile;
pub mod status;

use serde::Serialize;
use tauri::AppHandle;

pub use status::build_static_status;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhInstallProgress {
    pub state: String,
    pub message: String,
}

#[tauri::command]
pub fn get_pdf2zh_status(app: AppHandle) -> Result<status::Pdf2zhStatus, String> {
    Ok(status::build_static_status(&app)?.into_status())
}

#[tauri::command]
pub async fn install_pdf2zh_pack() -> Result<String, String> {
    Err("pdf2zh pack 下载器尚未接入。当前可设置 ROSETTA_PDF2ZH_BIN 指向本地 pdf2zh，或运行 rosetta-app/src-tauri/scripts/stage-pdf2zh-pack-local.sh 暂存本地 pack。".to_string())
}

#[tauri::command]
pub async fn cancel_pdf2zh_install() -> Result<String, String> {
    Ok("当前没有正在进行的 pdf2zh 安装任务。".to_string())
}

#[tauri::command]
pub async fn get_pdf2zh_install_progress() -> Result<Pdf2zhInstallProgress, String> {
    Ok(Pdf2zhInstallProgress {
        state: "idle".to_string(),
        message: "当前没有正在进行的 pdf2zh 安装任务。".to_string(),
    })
}
