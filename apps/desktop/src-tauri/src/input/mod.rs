#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::{
    keys::{KeyMode, Stroke},
    layout::Point,
    pipeline::{Button, InputEvent, Msg, PipelineHandle},
    session::KeyAccess,
    Result,
};
use std::sync::{
    atomic::{AtomicBool, Ordering::Relaxed},
    Arc,
};

#[derive(Default)]
pub(crate) struct Gate {
    enabled: AtomicBool,
    all_keys: AtomicBool,
    halo: AtomicBool,
}
impl Gate {
    pub(crate) fn set(&self, enabled: bool, mode: KeyMode, halo: bool) {
        self.all_keys.store(mode == KeyMode::All, Relaxed);
        self.halo.store(halo, Relaxed);
        self.enabled.store(enabled, Relaxed);
    }
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Relaxed)
    }
    pub(crate) fn mode(&self) -> KeyMode {
        if self.all_keys.load(Relaxed) {
            KeyMode::All
        } else {
            KeyMode::Shortcuts
        }
    }
}

pub(crate) struct Sink {
    pub(crate) gate: Arc<Gate>,
    pipeline: PipelineHandle,
}
impl Sink {
    pub(crate) fn new(gate: Arc<Gate>, pipeline: PipelineHandle) -> Self {
        Self { gate, pipeline }
    }
    pub(crate) fn key(&self, stroke: Stroke, cursor: Point) {
        self.pipeline
            .send(Msg::Input(InputEvent::Key { stroke, cursor }));
    }
    pub(crate) fn click(&self, button: Button, at: Point) {
        self.pipeline
            .send(Msg::Input(InputEvent::Click { button, at }));
    }
    pub(crate) fn moved(&self, at: Point) {
        if self.gate.halo.load(Relaxed) {
            self.pipeline.send(Msg::Input(InputEvent::Move { at }));
        }
    }
}

pub(crate) fn start(app: &tauri::AppHandle, sink: Sink) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::start(app, sink);
    #[cfg(target_os = "windows")]
    return {
        let _ = app;
        windows::start(sink)
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, sink);
        Err("Glassview supports macOS and Windows".into())
    }
}

pub(crate) fn key_access() -> KeyAccess {
    #[cfg(target_os = "macos")]
    return macos::key_access();
    #[cfg(not(target_os = "macos"))]
    KeyAccess::Granted
}

pub(crate) fn request_key_access(app: &tauri::AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::request_key_access(app);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}
