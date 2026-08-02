//! Linux backend: evdev input capture with a wlr-layer-shell overlay on Wayland
//! and an override-redirect ARGB window on X11.

#![cfg(target_os = "linux")]

mod keyboard;
mod pointer;
mod wayland;
mod x11;

use anyhow::Result;
use pc_core::Config;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Wayland,
    X11,
}

pub fn detect_session() -> Session {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Session::Wayland
    } else {
        Session::X11
    }
}

pub fn run(config: Config, config_path: Option<PathBuf>, session: Option<Session>) -> Result<()> {
    match session.unwrap_or_else(detect_session) {
        Session::Wayland => wayland::run(config, config_path),
        Session::X11 => x11::run(config, config_path),
    }
}
