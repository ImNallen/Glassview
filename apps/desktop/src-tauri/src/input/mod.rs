#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::{
    keys::{KeyMode, Mods, Stroke},
    layout::Point,
    pipeline::{Button, InputEvent, Msg, PipelineHandle, ScrollDelta},
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
    #[cfg(target_os = "linux")]
    pub(crate) fn halo(&self) -> bool {
        self.halo.load(Relaxed)
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
    pub(crate) fn press(&self, button: Button, mods: Mods, at: Point) {
        self.pipeline
            .send(Msg::Input(InputEvent::Press { button, mods, at }));
    }
    pub(crate) fn scroll(&self, at: Point, delta: ScrollDelta) {
        self.pipeline
            .send(Msg::Input(InputEvent::Scroll { at, delta }));
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
    #[cfg(target_os = "linux")]
    return {
        let _ = app;
        linux::start(sink)
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, sink);
        Err("Glassview supports macOS, Windows, and Linux".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    fn sink() -> (Sink, Receiver<Msg>) {
        let (sender, receiver) = mpsc::channel();
        (Sink::new(Arc::default(), PipelineHandle(sender)), receiver)
    }
    fn sent(receiver: &Receiver<Msg>) -> Vec<InputEvent> {
        receiver
            .try_iter()
            .map(|msg| match msg {
                Msg::Input(event) => event,
                _ => unreachable!("the sink only sends input"),
            })
            .collect()
    }
    #[test]
    fn motion_flows_only_while_the_halo_is_on() {
        let (sink, receiver) = sink();
        let at = Point { x: 1.0, y: 0.0 };
        sink.moved(at);
        assert_eq!(sent(&receiver), [], "halo off");
        sink.gate.set(true, KeyMode::Shortcuts, true);
        sink.moved(at);
        assert_eq!(sent(&receiver), [InputEvent::Move { at }]);
    }
}
