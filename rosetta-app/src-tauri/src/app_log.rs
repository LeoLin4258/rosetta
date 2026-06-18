//! App-wide log file.
//!
//! Redirects stderr to a log file so every `eprintln!` across all modules is
//! persisted to disk. The log lives at:
//!
//!   Windows: %APPDATA%/com.rosetta.desktop/logs/rosetta.log
//!   macOS:   ~/Library/Application Support/com.rosetta.desktop/logs/rosetta.log
//!
//! Old log files are rotated on startup: the previous run's log is renamed to
//! `rosetta.prev.log` (one generation only — keeps disk use bounded).

use std::fs;
use std::path::PathBuf;

const APP_ID: &str = "com.rosetta.desktop";

pub fn init() {
    let Some(log_dir) = logs_dir() else {
        return;
    };
    if let Err(error) = fs::create_dir_all(&log_dir) {
        eprintln!("[app-log] cannot create logs dir: {error}");
        return;
    }

    let log_path = log_dir.join("rosetta.log");
    let prev_path = log_dir.join("rosetta.prev.log");

    // Rotate: current → prev (overwrite previous prev).
    if log_path.exists() {
        let _ = fs::rename(&log_path, &prev_path);
    }

    match redirect_stderr_to_file(&log_path) {
        Ok(()) => {
            eprintln!("=== Rosetta log started ===");
            eprintln!("  log: {}", log_path.display());
            eprintln!(
                "  time: {}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            );
            eprintln!("  os: {}", std::env::consts::OS);
            eprintln!("  arch: {}", std::env::consts::ARCH);
        }
        Err(error) => {
            eprintln!("[app-log] failed to redirect stderr: {error}");
        }
    }
}

pub fn logs_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(APP_ID).join("logs"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support").join(APP_ID).join("logs"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

// Platform-specific stderr redirect.

#[cfg(target_os = "windows")]
fn redirect_stderr_to_file(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn SetStdHandle(nStdHandle: u32, hHandle: isize) -> i32;
    }
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4; // (DWORD)-12

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("open log file: {e}"))?;

    let raw = file.as_raw_handle() as isize;
    let ok = unsafe { SetStdHandle(STD_ERROR_HANDLE, raw) };
    if ok == 0 {
        return Err("SetStdHandle failed".to_string());
    }

    // Leak the file handle so it stays open for the app's lifetime.
    std::mem::forget(file);
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn redirect_stderr_to_file(path: &std::path::Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("open log file: {e}"))?;

    let fd = file.as_raw_fd();
    let result = unsafe { libc::dup2(fd, 2) };
    if result < 0 {
        return Err(format!(
            "dup2 failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    std::mem::forget(file);
    Ok(())
}
