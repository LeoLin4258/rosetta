//! Connectivity diagnostics for the managed RWKV runtime.
//!
//! The Windows beta.18 failure mode this protects against is below HTTP:
//! the sidecar can bind and listen on loopback, while same-machine TCP
//! connects to 127.0.0.1 / ::1 time out. Keep this module independent from
//! llama.cpp so it can prove whether the OS loopback path itself works.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::profile::RuntimeProfile;

#[cfg(target_os = "windows")]
use crate::windows_process::HideConsole;

const LOOPBACK_CONNECT_TIMEOUT: Duration = Duration::from_millis(1200);
const LOOPBACK_ACCEPT_GRACE: Duration = Duration::from_millis(250);
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(8);
const ELEVATED_REPAIR_TIMEOUT: Duration = Duration::from_secs(90);
const DEBUG_LOOPBACK_FAILURE_ENV: &str = "ROSETTA_DEBUG_MANAGED_RWKV_LOOPBACK_FAILURE";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectivityDiagnostics {
    pub platform: String,
    pub checked_at: String,
    pub target_profile_id: String,
    pub target_runtime_label: String,
    pub target_bind_host: String,
    pub target_loopback_ok: bool,
    pub loopback_ipv4_ok: bool,
    pub loopback_ipv6_ok: Option<bool>,
    pub probes: Vec<LoopbackProbe>,
    pub network_profiles: Vec<WindowsNetworkProfile>,
    pub firewall_profiles: Vec<WindowsFirewallProfile>,
    pub suspected_issue: Option<String>,
    pub message: String,
    pub recommended_actions: Vec<String>,
    pub powershell_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRuntimeConnectivityRepairResult {
    pub ok: bool,
    pub changed: bool,
    pub elevated: bool,
    pub message: String,
    pub diagnostics: ManagedRuntimeConnectivityDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopbackProbe {
    pub host: String,
    pub bind_ok: bool,
    pub connect_ok: bool,
    pub accepted: bool,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsNetworkProfile {
    pub name: Option<String>,
    pub interface_alias: Option<String>,
    pub network_category: Option<String>,
    pub ipv4_connectivity: Option<String>,
    pub ipv6_connectivity: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsFirewallProfile {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub default_inbound_action: Option<String>,
    pub default_outbound_action: Option<String>,
}

pub async fn collect_connectivity_diagnostics(
    profile: &'static RuntimeProfile,
) -> ManagedRuntimeConnectivityDiagnostics {
    if debug_loopback_failure_enabled() {
        return simulated_loopback_failure(profile);
    }

    let loopback = run_loopback_diagnostics(profile);
    let (network_profiles, firewall_profiles) = collect_windows_network_state().await;
    build_diagnostics(profile, loopback, network_profiles, firewall_profiles)
}

pub async fn repair_connectivity(
    profile: &'static RuntimeProfile,
) -> ManagedRuntimeConnectivityRepairResult {
    if debug_loopback_failure_enabled() {
        return ManagedRuntimeConnectivityRepairResult {
            ok: true,
            changed: true,
            elevated: false,
            message: "已修复本机连接。Rosetta 可以重新启动本地翻译引擎。".to_string(),
            diagnostics: simulated_loopback_repaired(profile),
        };
    }

    let before = collect_connectivity_diagnostics(profile).await;
    if before.target_loopback_ok {
        return ManagedRuntimeConnectivityRepairResult {
            ok: true,
            changed: false,
            elevated: false,
            message: "本机连接已经恢复，无需修复。".to_string(),
            diagnostics: before,
        };
    }

    let aliases = public_network_aliases(&before);
    if aliases.is_empty() {
        return ManagedRuntimeConnectivityRepairResult {
            ok: false,
            changed: false,
            elevated: false,
            message: "没有找到可自动修复的公用网络。请先完全退出安全软件或使用安全模式带网络测试。"
                .to_string(),
            diagnostics: before,
        };
    }

    let direct = set_network_profiles_private(&aliases, false).await;
    let (changed, elevated, repair_error) = match direct {
        Ok(()) => (true, false, None),
        Err(error) => match set_network_profiles_private(&aliases, true).await {
            Ok(()) => (true, true, None),
            Err(elevated_error) => (
                false,
                true,
                Some(format!("{error}; elevated repair failed: {elevated_error}")),
            ),
        },
    };

    let diagnostics = collect_connectivity_diagnostics(profile).await;
    let ok = diagnostics.target_loopback_ok;
    let message = if ok && changed {
        "已修复本机连接。Rosetta 可以重新启动本地翻译引擎。".to_string()
    } else if changed {
        "已尝试修复网络设置，但本机连接仍被 Windows 拦截。请完全退出安全软件或使用安全模式带网络测试。".to_string()
    } else {
        format!(
            "Rosetta 没能自动修复本机连接。{}",
            repair_error.unwrap_or_else(|| "请检查 Windows 网络和安全软件设置。".to_string())
        )
    };

    ManagedRuntimeConnectivityRepairResult {
        ok,
        changed,
        elevated,
        message,
        diagnostics,
    }
}

pub fn runtime_loopback_failure_hint(profile: &RuntimeProfile) -> Option<String> {
    if debug_loopback_failure_enabled() {
        return Some(format!(
            "\n\nRosetta 本机连接诊断: Windows 无法连接 {}。这通常不是模型或 runtime 问题，而是本机 loopback TCP 被系统防火墙、网络过滤/WFP 驱动或安全软件拦截。请在设置里的“本地运行时”点击“修复连接并重试”。",
            profile.bind_host
        ));
    }

    let loopback = run_loopback_diagnostics(profile);
    if loopback.target_loopback_ok {
        return None;
    }

    Some(format!(
        "\n\nRosetta 本机连接诊断: Windows 无法连接 {}。这通常不是模型或 runtime 问题，而是本机 loopback TCP 被系统防火墙、网络过滤/WFP 驱动或安全软件拦截。请在设置里的“本地运行时”执行连接诊断，或先检查当前网络是否为公用网络。",
        profile.bind_host
    ))
}

fn debug_loopback_failure_enabled() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    std::env::var(DEBUG_LOOPBACK_FAILURE_ENV)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn simulated_loopback_failure(
    profile: &'static RuntimeProfile,
) -> ManagedRuntimeConnectivityDiagnostics {
    build_diagnostics(
        profile,
        LoopbackDiagnostics {
            target_loopback_ok: false,
            ipv4_ok: false,
            ipv6_ok: Some(false),
            probes: vec![
                LoopbackProbe {
                    host: "127.0.0.1".to_string(),
                    bind_ok: true,
                    connect_ok: false,
                    accepted: false,
                    latency_ms: Some(1200),
                    error: Some("debug simulated loopback timeout".to_string()),
                },
                LoopbackProbe {
                    host: "::1".to_string(),
                    bind_ok: true,
                    connect_ok: false,
                    accepted: false,
                    latency_ms: Some(1200),
                    error: Some("debug simulated loopback timeout".to_string()),
                },
            ],
        },
        vec![WindowsNetworkProfile {
            name: Some("Debug Wi-Fi".to_string()),
            interface_alias: Some("WLAN".to_string()),
            network_category: Some("Public".to_string()),
            ipv4_connectivity: Some("Internet".to_string()),
            ipv6_connectivity: Some("NoTraffic".to_string()),
        }],
        vec![WindowsFirewallProfile {
            name: Some("Public".to_string()),
            enabled: Some(true),
            default_inbound_action: Some("Block".to_string()),
            default_outbound_action: Some("Allow".to_string()),
        }],
    )
}

fn simulated_loopback_repaired(
    profile: &'static RuntimeProfile,
) -> ManagedRuntimeConnectivityDiagnostics {
    build_diagnostics(
        profile,
        LoopbackDiagnostics {
            target_loopback_ok: true,
            ipv4_ok: true,
            ipv6_ok: Some(true),
            probes: vec![
                LoopbackProbe {
                    host: "127.0.0.1".to_string(),
                    bind_ok: true,
                    connect_ok: true,
                    accepted: true,
                    latency_ms: Some(1),
                    error: None,
                },
                LoopbackProbe {
                    host: "::1".to_string(),
                    bind_ok: true,
                    connect_ok: true,
                    accepted: true,
                    latency_ms: Some(1),
                    error: None,
                },
            ],
        },
        vec![WindowsNetworkProfile {
            name: Some("Debug Wi-Fi".to_string()),
            interface_alias: Some("WLAN".to_string()),
            network_category: Some("Private".to_string()),
            ipv4_connectivity: Some("Internet".to_string()),
            ipv6_connectivity: Some("NoTraffic".to_string()),
        }],
        vec![WindowsFirewallProfile {
            name: Some("Private".to_string()),
            enabled: Some(true),
            default_inbound_action: Some("Block".to_string()),
            default_outbound_action: Some("Allow".to_string()),
        }],
    )
}

fn build_diagnostics(
    profile: &'static RuntimeProfile,
    loopback: LoopbackDiagnostics,
    network_profiles: Vec<WindowsNetworkProfile>,
    firewall_profiles: Vec<WindowsFirewallProfile>,
) -> ManagedRuntimeConnectivityDiagnostics {
    let public_profiles = network_profiles
        .iter()
        .filter(|profile| {
            profile
                .network_category
                .as_deref()
                .is_some_and(|category| category.eq_ignore_ascii_case("Public"))
        })
        .collect::<Vec<_>>();

    let powershell_hint = public_profiles
        .first()
        .and_then(|profile| profile.interface_alias.as_deref())
        .map(|alias| {
            format!(
                "Set-NetConnectionProfile -InterfaceAlias \"{}\" -NetworkCategory Private",
                alias.replace('"', "`\"")
            )
        });

    let suspected_issue = if loopback.target_loopback_ok {
        None
    } else if !public_profiles.is_empty() {
        Some("windows-public-network-loopback-filtering".to_string())
    } else {
        Some("windows-loopback-tcp-blocked".to_string())
    };

    let message = if loopback.target_loopback_ok {
        format!(
            "本机 TCP loopback 可以连接 {}。如果 RWKV runtime 仍然不通，请继续查看 runtime 日志和端口占用。",
            profile.bind_host
        )
    } else if !public_profiles.is_empty() {
        format!(
            "Windows 无法连接 {}，并且当前网络包含 Public 配置。Rosetta 可能已经启动了本地翻译服务，但系统层拦住了 localhost 连接。",
            profile.bind_host
        )
    } else {
        format!(
            "Windows 无法连接 {}。这低于 HTTP 层，常见原因是防火墙、公用网络策略、安全软件或 WFP/TUN 网络过滤驱动。",
            profile.bind_host
        )
    };

    let mut recommended_actions = Vec::new();
    if loopback.target_loopback_ok {
        recommended_actions
            .push("重新启动本地运行时，并查看 runtime 日志里的最后 80 行。".to_string());
        recommended_actions.push("确认没有其他进程占用 Rosetta 分配的端口。".to_string());
    } else {
        if powershell_hint.is_some() {
            recommended_actions.push(
                "先把当前 Wi-Fi/以太网网络从 Public 改为 Private，然后重启 Rosetta 再试。"
                    .to_string(),
            );
        }
        recommended_actions.push(
            "如果安装过腾讯电脑管家、Clash/TUN、公司安全软件或网络加速器，请先完全退出或卸载它们做一次诊断验证。".to_string(),
        );
        recommended_actions.push(
            "仍然失败时，用“安全模式带网络”测试同样的 127.0.0.1 连接；安全模式恢复则基本指向第三方驱动或服务。".to_string(),
        );
        recommended_actions.push(
            "如果安全模式也失败，再执行 sfc /scannow 和 DISM /Online /Cleanup-Image /RestoreHealth 修复 Windows 网络栈。".to_string(),
        );
    }

    ManagedRuntimeConnectivityDiagnostics {
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        checked_at: crate::managed_rwkv::lifecycle::iso_now_for_diagnostics(),
        target_profile_id: profile.id.to_string(),
        target_runtime_label: profile.runtime_label.to_string(),
        target_bind_host: profile.bind_host.to_string(),
        target_loopback_ok: loopback.target_loopback_ok,
        loopback_ipv4_ok: loopback.ipv4_ok,
        loopback_ipv6_ok: loopback.ipv6_ok,
        probes: loopback.probes,
        network_profiles,
        firewall_profiles,
        suspected_issue,
        message,
        recommended_actions,
        powershell_hint,
    }
}

fn public_network_aliases(diagnostics: &ManagedRuntimeConnectivityDiagnostics) -> Vec<String> {
    diagnostics
        .network_profiles
        .iter()
        .filter(|profile| {
            profile
                .network_category
                .as_deref()
                .is_some_and(|category| category.eq_ignore_ascii_case("Public"))
        })
        .filter_map(|profile| profile.interface_alias.clone())
        .collect()
}

struct LoopbackDiagnostics {
    target_loopback_ok: bool,
    ipv4_ok: bool,
    ipv6_ok: Option<bool>,
    probes: Vec<LoopbackProbe>,
}

fn run_loopback_diagnostics(profile: &RuntimeProfile) -> LoopbackDiagnostics {
    let ipv4 = probe_tcp_loopback("127.0.0.1", IpAddr::V4(Ipv4Addr::LOCALHOST));
    let ipv6 = probe_tcp_loopback("::1", IpAddr::V6(Ipv6Addr::LOCALHOST));
    let ipv4_ok = probe_ok(&ipv4);
    let ipv6_ok = if ipv6.bind_ok {
        Some(probe_ok(&ipv6))
    } else {
        None
    };
    let target_loopback_ok = if profile.bind_host.contains("::1") {
        ipv6_ok.unwrap_or(false)
    } else {
        ipv4_ok
    };

    LoopbackDiagnostics {
        target_loopback_ok,
        ipv4_ok,
        ipv6_ok,
        probes: vec![ipv4, ipv6],
    }
}

fn probe_ok(probe: &LoopbackProbe) -> bool {
    probe.bind_ok && probe.connect_ok && probe.accepted
}

fn probe_tcp_loopback(label: &str, ip: IpAddr) -> LoopbackProbe {
    let listener = match TcpListener::bind(SocketAddr::new(ip, 0)) {
        Ok(listener) => listener,
        Err(error) => {
            return LoopbackProbe {
                host: label.to_string(),
                bind_ok: false,
                connect_ok: false,
                accepted: false,
                latency_ms: None,
                error: Some(format!("bind failed: {error}")),
            };
        }
    };

    if let Err(error) = listener.set_nonblocking(true) {
        return LoopbackProbe {
            host: label.to_string(),
            bind_ok: true,
            connect_ok: false,
            accepted: false,
            latency_ms: None,
            error: Some(format!("set_nonblocking failed: {error}")),
        };
    }

    let local_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(error) => {
            return LoopbackProbe {
                host: label.to_string(),
                bind_ok: true,
                connect_ok: false,
                accepted: false,
                latency_ms: None,
                error: Some(format!("read local addr failed: {error}")),
            };
        }
    };

    let started_at = Instant::now();
    let connector = thread::spawn(move || {
        TcpStream::connect_timeout(&local_addr, LOOPBACK_CONNECT_TIMEOUT)
            .map(|stream| {
                let _ = stream.shutdown(Shutdown::Both);
            })
            .map_err(|error| error.to_string())
    });

    let deadline = Instant::now() + LOOPBACK_CONNECT_TIMEOUT + LOOPBACK_ACCEPT_GRACE;
    let mut accepted = false;
    let mut accept_error = None;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.shutdown(Shutdown::Both);
                accepted = true;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                accept_error = Some(format!("accept failed: {error}"));
                break;
            }
        }
    }

    let connect_result = connector
        .join()
        .unwrap_or_else(|_| Err("connect thread panicked".to_string()));
    let connect_ok = connect_result.is_ok();
    let error = connect_result.err().or(accept_error);

    LoopbackProbe {
        host: label.to_string(),
        bind_ok: true,
        connect_ok,
        accepted,
        latency_ms: Some(started_at.elapsed().as_millis()),
        error,
    }
}

#[cfg(target_os = "windows")]
async fn collect_windows_network_state() -> (Vec<WindowsNetworkProfile>, Vec<WindowsFirewallProfile>)
{
    let network_profiles = powershell_json(
        "Get-NetConnectionProfile | Select-Object Name,InterfaceAlias,NetworkCategory,IPv4Connectivity,IPv6Connectivity | ConvertTo-Json -Compress",
    )
    .await
    .map(parse_network_profiles)
    .unwrap_or_default();

    let firewall_profiles = powershell_json(
        "Get-NetFirewallProfile | Select-Object Name,Enabled,DefaultInboundAction,DefaultOutboundAction | ConvertTo-Json -Compress",
    )
    .await
    .map(parse_firewall_profiles)
    .unwrap_or_default();

    (network_profiles, firewall_profiles)
}

#[cfg(not(target_os = "windows"))]
async fn collect_windows_network_state() -> (Vec<WindowsNetworkProfile>, Vec<WindowsFirewallProfile>)
{
    (Vec::new(), Vec::new())
}

#[cfg(target_os = "windows")]
async fn powershell_json(script: &str) -> Option<serde_json::Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .hide_console_on_windows()
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).ok()
}

fn parse_network_profiles(value: serde_json::Value) -> Vec<WindowsNetworkProfile> {
    normalize_json_array(value)
        .into_iter()
        .filter_map(|value| {
            Some(WindowsNetworkProfile {
                name: json_string(&value, "Name"),
                interface_alias: json_string(&value, "InterfaceAlias"),
                network_category: network_category(&value, "NetworkCategory"),
                ipv4_connectivity: json_string(&value, "IPv4Connectivity"),
                ipv6_connectivity: json_string(&value, "IPv6Connectivity"),
            })
        })
        .collect()
}

fn parse_firewall_profiles(value: serde_json::Value) -> Vec<WindowsFirewallProfile> {
    normalize_json_array(value)
        .into_iter()
        .map(|value| WindowsFirewallProfile {
            name: json_string(&value, "Name"),
            enabled: value.get("Enabled").and_then(|value| value.as_bool()),
            default_inbound_action: json_string(&value, "DefaultInboundAction"),
            default_outbound_action: json_string(&value, "DefaultOutboundAction"),
        })
        .collect()
}

fn normalize_json_array(value: serde_json::Value) -> Vec<serde_json::Value> {
    match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    })
}

fn network_category(value: &serde_json::Value, key: &str) -> Option<String> {
    let raw = value.get(key)?;
    match raw {
        serde_json::Value::String(text) => Some(match text.as_str() {
            "0" => "Public".to_string(),
            "1" => "Private".to_string(),
            "2" => "DomainAuthenticated".to_string(),
            other => other.to_string(),
        }),
        serde_json::Value::Number(number) => Some(match number.as_i64() {
            Some(0) => "Public".to_string(),
            Some(1) => "Private".to_string(),
            Some(2) => "DomainAuthenticated".to_string(),
            _ => number.to_string(),
        }),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
async fn set_network_profiles_private(aliases: &[String], elevated: bool) -> Result<(), String> {
    if aliases.is_empty() {
        return Ok(());
    }

    let script = network_repair_script(aliases);
    if elevated {
        run_elevated_powershell(&script).await
    } else {
        run_powershell(&script, POWERSHELL_TIMEOUT).await
    }
}

#[cfg(not(target_os = "windows"))]
async fn set_network_profiles_private(_aliases: &[String], _elevated: bool) -> Result<(), String> {
    Err("自动修复仅支持 Windows。".to_string())
}

fn network_repair_script(aliases: &[String]) -> String {
    let quoted_aliases = aliases
        .iter()
        .map(|alias| format!("'{}'", alias.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "$ErrorActionPreference = 'Stop'; $aliases = @({quoted_aliases}); foreach ($alias in $aliases) {{ Set-NetConnectionProfile -InterfaceAlias $alias -NetworkCategory Private }}"
    )
}

#[cfg(target_os = "windows")]
async fn run_powershell(script: &str, timeout: Duration) -> Result<(), String> {
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .hide_console_on_windows()
            .output(),
    )
    .await
    .map_err(|_| "PowerShell 操作超时。".to_string())?
    .map_err(|error| format!("无法启动 PowerShell: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

#[cfg(target_os = "windows")]
async fn run_elevated_powershell(script: &str) -> Result<(), String> {
    let script_path = std::env::temp_dir().join(format!(
        "rosetta-loopback-repair-{}.ps1",
        crate::managed_rwkv::lifecycle::iso_now_for_diagnostics().replace(':', "")
    ));
    std::fs::write(&script_path, script)
        .map_err(|error| format!("无法写入临时修复脚本: {error}"))?;

    let path = script_path.display().to_string().replace('\'', "''");
    let command = format!(
        "Start-Process -FilePath powershell -Verb RunAs -Wait -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File','{path}')"
    );
    let result = run_powershell(&command, ELEVATED_REPAIR_TIMEOUT).await;
    let _ = std::fs::remove_file(script_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_rwkv::profile::WINDOWS_AMD64_LLAMACPP_VULKAN;

    #[test]
    fn public_network_profile_adds_private_network_hint() {
        let diagnostics = build_diagnostics(
            &WINDOWS_AMD64_LLAMACPP_VULKAN,
            LoopbackDiagnostics {
                target_loopback_ok: false,
                ipv4_ok: false,
                ipv6_ok: Some(false),
                probes: Vec::new(),
            },
            vec![WindowsNetworkProfile {
                name: Some("Office Wi-Fi".to_string()),
                interface_alias: Some("WLAN".to_string()),
                network_category: Some("Public".to_string()),
                ipv4_connectivity: None,
                ipv6_connectivity: None,
            }],
            Vec::new(),
        );

        assert_eq!(
            diagnostics.suspected_issue.as_deref(),
            Some("windows-public-network-loopback-filtering")
        );
        assert!(diagnostics
            .powershell_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("Set-NetConnectionProfile")));
    }

    #[test]
    fn numeric_network_category_is_human_readable() {
        let value = serde_json::json!({
            "Name": "Office",
            "InterfaceAlias": "WLAN",
            "NetworkCategory": 0,
        });
        let profiles = parse_network_profiles(value);
        assert_eq!(profiles[0].network_category.as_deref(), Some("Public"));
    }

    #[test]
    fn target_ok_reports_runtime_level_next_step() {
        let diagnostics = build_diagnostics(
            &WINDOWS_AMD64_LLAMACPP_VULKAN,
            LoopbackDiagnostics {
                target_loopback_ok: true,
                ipv4_ok: true,
                ipv6_ok: Some(true),
                probes: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(diagnostics.suspected_issue, None);
        assert!(diagnostics.message.contains("可以连接"));
    }
}
