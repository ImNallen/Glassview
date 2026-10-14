use crate::{
    keys::{Mods, Os, Stroke, StrokeView},
    layout::{Layout, Point},
    overlays::Surface,
};
use serde::Serialize;
use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};
use tauri::Emitter;

const TICK: Duration = Duration::from_millis(16);
const MIN_SCROLL: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}
impl Button {
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn from_mac_number(number: i64) -> Option<Self> {
        match number {
            2 => Some(Self::Middle),
            3 => Some(Self::Back),
            4 => Some(Self::Forward),
            _ => None,
        }
    }
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn from_x_button(x: u16) -> Option<Self> {
        match x {
            1 => Some(Self::Back),
            2 => Some(Self::Forward),
            _ => None,
        }
    }
}

/// Positive `dy` scrolls toward the top of the document and positive `dx` toward its
/// right, whatever the OS's natural scrolling setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollDelta {
    pub(crate) dx: f64,
    pub(crate) dy: f64,
}

/// The way the view moves through the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}
impl ScrollDirection {
    fn of(ScrollDelta { dx, dy }: ScrollDelta) -> Option<Self> {
        let direction = match (dy.abs() >= dx.abs(), dy > 0.0, dx > 0.0) {
            (true, true, _) => Self::Up,
            (true, false, _) => Self::Down,
            (false, _, true) => Self::Right,
            (false, _, false) => Self::Left,
        };
        (dx.abs().max(dy.abs()) >= MIN_SCROLL).then_some(direction)
    }
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) enum InputEvent {
    Press {
        button: Button,
        mods: Mods,
        at: Point,
    },
    Move {
        at: Point,
    },
    Scroll {
        at: Point,
        delta: ScrollDelta,
    },
    Key {
        stroke: Stroke,
        cursor: Point,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Config {
    pub(crate) enabled: bool,
    pub(crate) halo: bool,
}

pub(crate) enum Msg {
    Input(InputEvent),
    Config(Config),
    Layout(Layout),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum OverlayEvent {
    Click {
        button: Button,
        mods: Option<String>,
        x: f64,
        y: f64,
    },
    Halo {
        at: Option<Point>,
    },
    Scroll {
        x: f64,
        y: f64,
        direction: ScrollDirection,
    },
    Key {
        stroke: StrokeView,
    },
}

#[derive(Debug, PartialEq)]
pub(crate) struct Emit {
    pub(crate) overlay: usize,
    pub(crate) event: OverlayEvent,
}

pub(crate) struct Pipeline {
    os: Os,
    config: Config,
    layout: Layout,
    halo_overlay: Option<usize>,
    pending_move: Option<Point>,
    /// Deltas summed since the last tick, at the latest pointer position.
    pending_scroll: Option<(Point, ScrollDelta)>,
    last_tick: Option<Instant>,
}

impl Pipeline {
    pub(crate) fn new(os: Os, config: Config, layout: Layout) -> Self {
        Self {
            os,
            config,
            layout,
            halo_overlay: None,
            pending_move: None,
            pending_scroll: None,
            last_tick: None,
        }
    }

    pub(crate) fn handle(&mut self, msg: Msg, now: Instant) -> Vec<Emit> {
        match msg {
            Msg::Config(config) => {
                self.config = config;
                if !config.enabled {
                    self.pending_scroll = None;
                }
                if !(config.enabled && config.halo) {
                    self.pending_move = None;
                    return self.clear_halo();
                }
                vec![]
            }
            Msg::Layout(layout) => {
                self.layout = layout;
                self.pending_move = None;
                self.pending_scroll = None;
                self.clear_halo()
            }
            Msg::Input(_) if !self.config.enabled => vec![],
            Msg::Input(InputEvent::Press { button, mods, at }) => {
                // A move still waiting for its tick is older than the press.
                self.pending_move = self.pending_move.map(|_| at);
                self.layout
                    .locate(at)
                    .map(|(overlay, css)| Emit {
                        overlay,
                        event: OverlayEvent::Click {
                            button,
                            mods: mods.label(self.os),
                            x: css.x,
                            y: css.y,
                        },
                    })
                    .into_iter()
                    .collect()
            }
            Msg::Input(InputEvent::Move { at }) => {
                if !self.config.halo {
                    return vec![];
                }
                self.pending_move = Some(at);
                self.tick(now)
            }
            Msg::Input(InputEvent::Scroll { at, delta }) => {
                let sum = self.pending_scroll.map_or(delta, |(_, sum)| ScrollDelta {
                    dx: sum.dx + delta.dx,
                    dy: sum.dy + delta.dy,
                });
                self.pending_scroll = Some((at, sum));
                self.tick(now)
            }
            Msg::Input(InputEvent::Key { stroke, cursor }) => {
                if self.layout.displays().is_empty() {
                    return vec![];
                }
                let overlay = self.layout.locate(cursor).map_or(0, |(overlay, _)| overlay);
                vec![Emit {
                    overlay,
                    event: OverlayEvent::Key {
                        stroke: stroke.view(self.os),
                    },
                }]
            }
        }
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        let pending = self.pending_move.is_some() || self.pending_scroll.is_some();
        self.last_tick.filter(|_| pending).map(|last| last + TICK)
    }

    fn tick(&mut self, now: Instant) -> Vec<Emit> {
        match self.deadline() {
            Some(deadline) if now < deadline => vec![],
            _ => self.flush(now),
        }
    }

    pub(crate) fn flush(&mut self, now: Instant) -> Vec<Emit> {
        if self.pending_move.is_none() && self.pending_scroll.is_none() {
            return vec![];
        }
        self.last_tick = Some(now);
        let mut emits = Vec::new();
        if let Some(at) = self.pending_move.take() {
            let located = self.layout.locate(at);
            emits.extend(follow(
                &mut self.halo_overlay,
                located.map(|(overlay, css)| (overlay, OverlayEvent::Halo { at: Some(css) })),
                OverlayEvent::Halo { at: None },
            ));
        }
        if let Some((at, delta)) = self.pending_scroll.take() {
            let located = self.layout.locate(at);
            emits.extend(ScrollDirection::of(delta).zip(located).map(
                |(direction, (overlay, css))| Emit {
                    overlay,
                    event: OverlayEvent::Scroll {
                        x: css.x,
                        y: css.y,
                        direction,
                    },
                },
            ));
        }
        emits
    }

    fn clear_halo(&mut self) -> Vec<Emit> {
        follow(
            &mut self.halo_overlay,
            None,
            OverlayEvent::Halo { at: None },
        )
    }
}

/// Moves something drawn on one overlay to `shown`, clearing the overlay it leaves
/// first, so an overlay never sees the next display's `Some` before its own `None`.
fn follow(
    current: &mut Option<usize>,
    shown: Option<(usize, OverlayEvent)>,
    cleared: OverlayEvent,
) -> Vec<Emit> {
    let next = shown.as_ref().map(|(overlay, _)| *overlay);
    let left = current.filter(|&old| Some(old) != next);
    *current = next;
    left.map(|overlay| Emit {
        overlay,
        event: cleared,
    })
    .into_iter()
    .chain(shown.map(|(overlay, event)| Emit { overlay, event }))
    .collect()
}

#[derive(Clone)]
pub(crate) struct PipelineHandle(pub(crate) Sender<Msg>);
impl PipelineHandle {
    pub(crate) fn send(&self, msg: Msg) {
        let _ = self.0.send(msg);
    }
}

pub(crate) fn start(app: tauri::AppHandle, mut pipeline: Pipeline) -> PipelineHandle {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("glassview-pipeline".into())
        .spawn(move || loop {
            let received = match pipeline.deadline() {
                Some(deadline) => {
                    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    {
                        Ok(msg) => Some(msg),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match receiver.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => break,
                },
            };
            let emits = match received {
                Some(msg) => pipeline.handle(msg, Instant::now()),
                None => pipeline.flush(Instant::now()),
            };
            for Emit { overlay, event } in emits {
                // Tauri evaluates a named event in every webview listening on that name, even ones
                // the target excludes, so the name is the overlay's own label rather than a shared one.
                let label = Surface::Overlay(overlay).label();
                if let Err(error) = app.emit_to(&label, &label, event) {
                    log::warn!("Could not update overlay {overlay}: {error}");
                }
            }
        })
        .expect("the pipeline thread starts");
    PipelineHandle(sender)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        keys::{admit, Key, KeyMode, KeyPress, Mods},
        layout::{InputSpace, MonitorInfo},
    };

    const ON: Config = Config {
        enabled: true,
        halo: true,
    };
    fn at(x: f64, y: f64) -> Point {
        Point { x, y }
    }
    fn two_displays() -> Layout {
        Layout::from_monitors(
            &[
                MonitorInfo {
                    position: (0, 0),
                    size: (1000, 800),
                    scale: 1.0,
                },
                MonitorInfo {
                    position: (1000, 0),
                    size: (1000, 800),
                    scale: 2.0,
                },
            ],
            InputSpace::Physical,
        )
    }
    fn halo(overlay: usize, at: Option<Point>) -> Emit {
        Emit {
            overlay,
            event: OverlayEvent::Halo { at },
        }
    }
    fn input(event: InputEvent) -> Msg {
        Msg::Input(event)
    }
    fn hover(x: f64, y: f64) -> Msg {
        input(InputEvent::Move { at: at(x, y) })
    }
    fn press(button: Button, mods: Mods, x: f64, y: f64) -> Msg {
        input(InputEvent::Press {
            button,
            mods,
            at: at(x, y),
        })
    }
    fn click(overlay: usize, button: Button, mods: Option<&str>, x: f64, y: f64) -> Emit {
        Emit {
            overlay,
            event: OverlayEvent::Click {
                button,
                mods: mods.map(Into::into),
                x,
                y,
            },
        }
    }
    const HALO_OFF: Config = Config {
        enabled: true,
        halo: false,
    };
    fn command_p() -> Stroke {
        let press = KeyPress {
            mods: Mods::META | Mods::SHIFT,
            key: Key::Char('p'),
            text: None,
        };
        admit(press, KeyMode::Shortcuts).unwrap()
    }

    #[test]
    fn a_thousand_moves_in_100_ms_emit_seven_halos_then_flush_the_last() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        let mut emitted = Vec::new();
        for i in 0..1000u32 {
            let now = start + Duration::from_micros(u64::from(i) * 100);
            let x = f64::from(i % 900);
            emitted.extend(pipeline.handle(hover(x, 10.0), now));
        }
        assert_eq!(emitted.len(), 7);
        assert_eq!(emitted[0], halo(0, Some(at(0.0, 10.0))));
        assert_eq!(
            pipeline.deadline(),
            Some(start + Duration::from_millis(112)),
            "the last move waits for the next slot"
        );
        assert_eq!(
            pipeline.flush(start + Duration::from_millis(112)),
            [halo(0, Some(at(99.0, 10.0)))]
        );
        assert_eq!(pipeline.deadline(), None);
    }

    #[test]
    fn the_halo_moves_between_displays_and_clears_when_turned_off() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        assert_eq!(
            pipeline.handle(hover(990.0, 10.0), start),
            [halo(0, Some(at(990.0, 10.0)))]
        );
        assert_eq!(
            pipeline.handle(hover(1010.0, 10.0), start + TICK),
            [halo(0, None), halo(1, Some(at(5.0, 5.0)))]
        );
        assert_eq!(
            pipeline.handle(hover(1010.0, 900.0), start + TICK * 2),
            [halo(1, None)],
            "below every display"
        );
        assert_eq!(
            pipeline.handle(hover(1010.0, 10.0), start + TICK * 3),
            [halo(1, Some(at(5.0, 5.0)))]
        );
        assert_eq!(
            pipeline.handle(Msg::Config(HALO_OFF), start),
            [halo(1, None)]
        );
        let later = start + Duration::from_secs(1);
        assert_eq!(pipeline.handle(hover(10.0, 10.0), later), []);
    }

    #[test]
    fn disabling_clears_the_halo_and_drops_input_in_flight() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        pipeline.handle(hover(1.0, 1.0), start);
        let disabled = Config {
            enabled: false,
            halo: true,
        };
        assert_eq!(
            pipeline.handle(Msg::Config(disabled), start),
            [halo(0, None)]
        );
        assert_eq!(
            pipeline.handle(press(Button::Left, Mods::NONE, 5.0, 5.0), start),
            []
        );
        let key = InputEvent::Key {
            stroke: command_p(),
            cursor: at(5.0, 5.0),
        };
        assert_eq!(pipeline.handle(input(key), start), []);
    }

    #[test]
    fn clicks_land_in_the_css_pixels_of_their_display() {
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        assert_eq!(
            pipeline.handle(
                press(Button::Right, Mods::NONE, 1500.0, 400.0),
                Instant::now()
            ),
            [click(1, Button::Right, None, 250.0, 200.0)]
        );
        assert_eq!(
            pipeline.handle(
                press(Button::Left, Mods::NONE, 500.0, 900.0),
                Instant::now()
            ),
            [],
            "below every display"
        );
    }

    #[test]
    fn a_press_ripples_with_its_modifiers() {
        let now = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        let shift_command = Mods::SHIFT | Mods::META;
        assert_eq!(
            pipeline.handle(press(Button::Left, shift_command, 100.0, 50.0), now),
            [click(0, Button::Left, Some("⇧⌘"), 100.0, 50.0)]
        );
        let mut windows = Pipeline::new(Os::Windows, HALO_OFF, two_displays());
        assert_eq!(
            windows.handle(press(Button::Middle, Mods::CTRL, 10.0, 10.0), now),
            [click(0, Button::Middle, Some("Ctrl"), 10.0, 10.0)]
        );
    }

    #[test]
    fn a_press_supersedes_a_hover_waiting_for_its_tick() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        pipeline.handle(hover(10.0, 10.0), start);
        pipeline.handle(hover(20.0, 10.0), start + Duration::from_millis(1));
        pipeline.handle(
            press(Button::Left, Mods::NONE, 30.0, 10.0),
            start + Duration::from_millis(2),
        );
        assert_eq!(
            pipeline.flush(start + TICK),
            [halo(0, Some(at(30.0, 10.0)))],
            "the halo never jumps back to where the pointer was before the press"
        );
    }

    #[test]
    fn scrolling_shows_one_direction_per_tick() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        let mut scroll = |dx, dy, ms| {
            let delta = ScrollDelta { dx, dy };
            let at = at(1500.0, 400.0);
            pipeline.handle(
                input(InputEvent::Scroll { at, delta }),
                start + Duration::from_millis(ms),
            )
        };
        let shown = |direction| Emit {
            overlay: 1,
            event: OverlayEvent::Scroll {
                x: 250.0,
                y: 200.0,
                direction,
            },
        };
        assert_eq!(scroll(0.0, 3.0, 0), [shown(ScrollDirection::Up)]);
        for ms in 1..=10 {
            assert_eq!(scroll(0.0, -0.5, ms), []);
        }
        assert_eq!(
            scroll(0.0, 0.0, 16),
            [shown(ScrollDirection::Down)],
            "small deltas add up within a tick"
        );
        assert_eq!(scroll(4.0, -1.0, 40), [shown(ScrollDirection::Right)]);
        assert_eq!(scroll(-4.0, 1.0, 60), [shown(ScrollDirection::Left)]);
        assert_eq!(scroll(0.6, 0.0, 70), []);
        assert_eq!(scroll(0.6, 0.0, 75), []);
        assert_eq!(scroll(0.0, 0.0, 76), [shown(ScrollDirection::Right)]);
        assert_eq!(scroll(0.0, 0.4, 100), [], "trackpad jitter");
        assert_eq!(pipeline.deadline(), None);
    }

    #[test]
    fn disabling_drops_a_scroll_waiting_for_its_tick() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        let scroll = || {
            input(InputEvent::Scroll {
                at: at(10.0, 10.0),
                delta: ScrollDelta { dx: 0.0, dy: 5.0 },
            })
        };
        pipeline.handle(scroll(), start);
        pipeline.handle(scroll(), start + Duration::from_millis(1));
        let disabled = Config {
            enabled: false,
            halo: false,
        };
        pipeline.handle(Msg::Config(disabled), start);
        assert_eq!(pipeline.flush(start + TICK), []);
    }

    #[test]
    fn extra_buttons_decode_on_each_os() {
        assert_eq!(Button::from_mac_number(2), Some(Button::Middle));
        assert_eq!(Button::from_mac_number(3), Some(Button::Back));
        assert_eq!(Button::from_mac_number(4), Some(Button::Forward));
        assert_eq!(Button::from_mac_number(5), None);
        assert_eq!(Button::from_x_button(1), Some(Button::Back));
        assert_eq!(Button::from_x_button(2), Some(Button::Forward));
        assert_eq!(Button::from_x_button(0), None);
    }

    #[test]
    fn keys_go_to_the_display_under_the_cursor_with_the_os_label() {
        let mut pipeline = Pipeline::new(Os::Windows, ON, two_displays());
        let key = |x| {
            input(InputEvent::Key {
                stroke: command_p(),
                cursor: at(x, 10.0),
            })
        };
        let chord = |overlay| Emit {
            overlay,
            event: OverlayEvent::Key {
                stroke: StrokeView::Chord {
                    label: "Win+Shift+P".into(),
                },
            },
        };
        assert_eq!(pipeline.handle(key(1200.0), Instant::now()), [chord(1)]);
        assert_eq!(
            pipeline.handle(key(-50.0), Instant::now()),
            [chord(0)],
            "outside every display"
        );
        pipeline.handle(Msg::Layout(Layout::default()), Instant::now());
        assert_eq!(pipeline.handle(key(10.0), Instant::now()), []);
    }

    #[test]
    fn overlay_events_keep_their_wire_shape() {
        let json = |event: OverlayEvent| serde_json::to_value(event).unwrap();
        assert_eq!(
            json(OverlayEvent::Click {
                button: Button::Middle,
                mods: Some("⌥".into()),
                x: 1.5,
                y: 2.0
            }),
            serde_json::json!({"kind": "click", "button": "middle", "mods": "⌥", "x": 1.5, "y": 2.0})
        );
        assert_eq!(
            json(OverlayEvent::Scroll {
                x: 1.0,
                y: 2.0,
                direction: ScrollDirection::Down
            }),
            serde_json::json!({"kind": "scroll", "x": 1.0, "y": 2.0, "direction": "down"})
        );
        assert_eq!(
            json(OverlayEvent::Halo { at: None }),
            serde_json::json!({"kind": "halo", "at": null})
        );
        assert_eq!(
            json(OverlayEvent::Key {
                stroke: StrokeView::Text { text: "j".into() }
            }),
            serde_json::json!({"kind": "key", "stroke": {"kind": "text", "text": "j"}})
        );
    }
}
