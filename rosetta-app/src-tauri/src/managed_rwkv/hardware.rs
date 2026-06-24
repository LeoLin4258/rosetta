use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::windows_process::HideConsole;

use super::profile::{RuntimeLaunchKind, RuntimeProfile};

const MIN_COMPUTE_CAPABILITY: (u32, u32) = (7, 5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSupport {
    pub supported: bool,
    pub gpu_name: Option<String>,
    pub compute_capability: Option<String>,
    pub message: String,
}

pub fn inspect(profile: &RuntimeProfile) -> HardwareSupport {
    if profile.platform_os != "windows" || profile.launch_kind == RuntimeLaunchKind::LlamaCppServer
    {
        return HardwareSupport {
            supported: true,
            gpu_name: None,
            compute_capability: None,
            message: profile.hardware_requirement.to_string(),
        };
    }
    inspect_windows_nvidia()
}

pub fn ensure_supported(profile: &RuntimeProfile) -> Result<HardwareSupport, String> {
    let result = inspect(profile);
    if result.supported {
        Ok(result)
    } else {
        Err(result.message)
    }
}

fn inspect_windows_nvidia() -> HardwareSupport {
    let Some(executable) = locate_nvidia_smi() else {
        return unsupported(
            None,
            None,
            "未检测到 NVIDIA 驱动。Windows 版 Rosetta 需要 NVIDIA GPU，且计算能力不低于 SM75。",
        );
    };

    let output = Command::new(executable)
        .args([
            "--query-gpu=name,compute_cap",
            "--format=csv,noheader,nounits",
        ])
        .hide_console_on_windows()
        .output();
    let Ok(output) = output else {
        return unsupported(
            None,
            None,
            "无法读取 NVIDIA GPU 信息。请确认显卡驱动已正确安装。",
        );
    };
    if !output.status.success() {
        return unsupported(
            None,
            None,
            "NVIDIA 驱动未能返回 GPU 信息。请更新驱动后重试。",
        );
    }

    let mut detected = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some((name, capability)) = line.rsplit_once(',') else {
            continue;
        };
        let capability = capability.trim();
        if let Some(parsed) = parse_compute_capability(capability) {
            detected.push((name.trim().to_string(), capability.to_string(), parsed));
        }
    }

    if let Some((name, capability, _)) = detected
        .iter()
        .find(|(_, _, parsed)| *parsed >= MIN_COMPUTE_CAPABILITY)
    {
        return HardwareSupport {
            supported: true,
            gpu_name: Some(name.clone()),
            compute_capability: Some(capability.clone()),
            message: format!("已检测到 {name}（SM{}）。", capability.replace('.', "")),
        };
    }

    if let Some((name, capability, _)) = detected.first() {
        return unsupported(
            Some(name.clone()),
            Some(capability.clone()),
            format!(
                "检测到 {name}（计算能力 {capability}），但 Windows 版 Rosetta 需要 SM75 或更新的 NVIDIA GPU。"
            ),
        );
    }

    unsupported(
        None,
        None,
        "未检测到可用的 NVIDIA CUDA GPU。Windows 版 Rosetta 仅支持 SM75 或更新的 NVIDIA GPU。",
    )
}

fn locate_nvidia_smi() -> Option<PathBuf> {
    let fixed = [
        PathBuf::from(r"C:\Windows\System32\nvidia-smi.exe"),
        PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe"),
    ];
    if let Some(found) = fixed.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    // Fall back to PATH lookup via `where.exe`.
    Command::new("where.exe")
        .arg("nvidia-smi.exe")
        .hide_console_on_windows()
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|line| PathBuf::from(line.trim()))
        })
        .filter(|p| p.is_file())
}

fn unsupported(
    gpu_name: Option<String>,
    compute_capability: Option<String>,
    message: impl Into<String>,
) -> HardwareSupport {
    HardwareSupport {
        supported: false,
        gpu_name,
        compute_capability,
        message: message.into(),
    }
}

fn parse_compute_capability(value: &str) -> Option<(u32, u32)> {
    let (major, minor) = value.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::parse_compute_capability;

    #[test]
    fn parses_compute_capability() {
        assert_eq!(parse_compute_capability("7.5"), Some((7, 5)));
        assert_eq!(parse_compute_capability("12.0"), Some((12, 0)));
        assert_eq!(parse_compute_capability("unknown"), None);
    }
}
