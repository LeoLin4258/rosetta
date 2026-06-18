//! Helpers shared by child processes that must stay invisible on Windows.

#![allow(dead_code)]

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) trait HideConsole {
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
