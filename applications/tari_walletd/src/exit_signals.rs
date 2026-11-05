// Copyright 2026 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

//! The OS signals that ask walletd to stop: SIGTERM, which `docker stop` sends, Ctrl-C, and on Windows the console
//! closing or the system shutting down.

use std::io;

#[cfg(unix)]
pub struct ExitSignals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ExitSignals {
    pub fn register() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    /// Waits for the next exit signal, and returns its name.
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.terminate.recv() => "SIGTERM",
            _ = self.interrupt.recv() => "SIGINT",
        }
    }
}

#[cfg(windows)]
pub struct ExitSignals {
    ctrl_c: tokio::signal::windows::CtrlC,
    ctrl_close: tokio::signal::windows::CtrlClose,
    ctrl_shutdown: tokio::signal::windows::CtrlShutdown,
}

#[cfg(windows)]
impl ExitSignals {
    pub fn register() -> io::Result<Self> {
        Ok(Self {
            ctrl_c: tokio::signal::windows::ctrl_c()?,
            ctrl_close: tokio::signal::windows::ctrl_close()?,
            ctrl_shutdown: tokio::signal::windows::ctrl_shutdown()?,
        })
    }

    /// Waits for the next exit signal, and returns its name.
    pub async fn recv(&mut self) -> &'static str {
        tokio::select! {
            _ = self.ctrl_c.recv() => "Ctrl-C",
            _ = self.ctrl_close.recv() => "CTRL_CLOSE",
            _ = self.ctrl_shutdown.recv() => "CTRL_SHUTDOWN",
        }
    }
}
