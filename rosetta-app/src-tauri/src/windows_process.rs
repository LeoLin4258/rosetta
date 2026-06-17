//! Helpers for spawning child processes without flashing a console window on
//! Windows.
//!
//! Without `CREATE_NO_WINDOW`, every console-subsystem child we launch (tar,
//! taskkill, python.exe, the RWKV sidecar, …) inherits / allocates a new
//! conhost window that pops up on screen. That alone is ugly; worse, when the
//! sidecar's stdout/stderr is left bound to the inherited console and the
//! child fills the console's pipe buffer, writes block and the sidecar
//! freezes mid-translation. Both symptoms vanish once we strip the console.
//!
//! Call `hide_console_on_windows(&mut command)` right before `.spawn()` /
//! `.status()` / `.output()` on every Windows-bound process.

#![allow(dead_code)]

/// Equivalent to the Windows `CREATE_NO_WINDOW` process creation flag (0x08000000).
#[cfg(windows)]
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) trait HideConsole {
    /// Set `CREATE_NO_WINDOW` on Windows; no-op everywhere else.
    fn hide_console_on_windows(&mut self) -> &mut Self;
}

impl HideConsole for std::process::Command {
    #[cfg(windows)]
    fn hide_console_on_windows(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn hide_console_on_windows(&mut self) -> &mut Self {
        self
    }
}

impl HideConsole for tokio::process::Command {
    #[cfg(windows)]
    fn hide_console_on_windows(&mut self) -> &mut Self {
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn hide_console_on_windows(&mut self) -> &mut Self {
        self
    }
}
