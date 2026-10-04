#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::{
    keys::{KeyMode, Mods, Stroke},
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
    /// Set by a press and cleared by the first motion with no button down, so motion
    /// flows while the halo is off for as long as a hold could be in progress.
    held: AtomicBool,
}
impl Sink {
    pub(crate) fn new(gate: Arc<Gate>, pipeline: PipelineHandle) -> Self {
        Self {
            gate,
            pipeline,
            held: AtomicBool::new(false),
        }
    }
    pub(crate) fn key(&self, stroke: Stroke, cursor: Point) {
        self.pipeline
            .send(Msg::Input(InputEvent::Key { stroke, cursor }));
    }
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn holding(&self) -> bool {
        self.held.load(Relaxed)
    }
    pub(crate) fn press(&self, button: Button, mods: Mods, at: Point) {
        self.held.store(true, Relaxed);
        self.pipeline
            .send(Msg::Input(InputEvent::Press { button, mods, at }));
    }
    pub(crate) fn release(&self, button: Button, at: Point) {
        self.pipeline
            .send(Msg::Input(InputEvent::Release { button, at }));
    }
    pub(crate) fn moved(&self, at: Point, buttons_down: bool) {
        if !(self.gate.halo.load(Relaxed) || self.held.load(Relaxed) || buttons_down) {
            return;
        }
        if !buttons_down {
            self.held.store(false, Relaxed);
        }
        self.pipeline
            .send(Msg::Input(InputEvent::Move { at, buttons_down }));
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
    fn moved(x: f64, buttons_down: bool) -> InputEvent {
        InputEvent::Move {
            at: Point { x, y: 0.0 },
            buttons_down,
        }
    }

    #[test]
    fn motion_with_the_halo_off_flows_only_while_a_button_may_be_held() {
        let (sink, receiver) = sink();
        sink.moved(Point { x: 1.0, y: 0.0 }, false);
        assert_eq!(sent(&receiver), [], "nothing held");
        sink.press(Button::Left, Mods::SHIFT, Point { x: 2.0, y: 0.0 });
        sink.moved(Point { x: 3.0, y: 0.0 }, true);
        sink.release(Button::Left, Point { x: 4.0, y: 0.0 });
        sink.moved(Point { x: 5.0, y: 0.0 }, false);
        sink.moved(Point { x: 6.0, y: 0.0 }, false);
        assert_eq!(
            sent(&receiver),
            [
                InputEvent::Press {
                    button: Button::Left,
                    mods: Mods::SHIFT,
                    at: Point { x: 2.0, y: 0.0 }
                },
                moved(3.0, true),
                InputEvent::Release {
                    button: Button::Left,
                    at: Point { x: 4.0, y: 0.0 }
                },
                moved(5.0, false),
            ],
            "the first motion with no button down is the last one sent"
        );
        assert!(!sink.holding());
    }

    #[test]
    fn a_drag_whose_press_was_missed_still_flows() {
        let (sink, receiver) = sink();
        sink.moved(Point { x: 1.0, y: 0.0 }, true);
        assert_eq!(sent(&receiver), [moved(1.0, true)]);
    }

    #[test]
    fn motion_with_the_halo_on_always_flows() {
        let (sink, receiver) = sink();
        sink.gate.set(true, KeyMode::Shortcuts, true);
        sink.moved(Point { x: 1.0, y: 0.0 }, false);
        assert_eq!(sent(&receiver), [moved(1.0, false)]);
    }
}
