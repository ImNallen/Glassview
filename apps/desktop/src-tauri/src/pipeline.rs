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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Button {
    Left,
    Right,
    Middle,
}
impl Button {
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn from_mac_number(number: i64) -> Option<Self> {
        match number {
            2 => Some(Self::Middle),
            _ => None,
        }
    }
}

#[cfg_attr(test, derive(Debug, PartialEq))]
pub(crate) enum InputEvent {
    Press {
        button: Button,
        mods: Mods,
        at: Point,
    },
    Release {
        button: Button,
        at: Point,
    },
    /// `buttons_down` is the OS's word on whether any button is held, so a release
    /// the capture layer never saw still ends the hold.
    Move {
        at: Point,
        buttons_down: bool,
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
pub(crate) struct HoldView {
    pub(crate) button: Button,
    pub(crate) mods: Option<String>,
    pub(crate) x: f64,
    pub(crate) y: f64,
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
    /// A snapshot like `Halo`: `None` means nothing is held on this overlay, and the
    /// first `Some` after it starts a new path.
    Hold {
        hold: Option<HoldView>,
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

/// The one button being held. A second button pressed meanwhile only ripples.
struct Hold {
    button: Button,
    mods: Mods,
    overlay: Option<usize>,
}

pub(crate) struct Pipeline {
    os: Os,
    config: Config,
    layout: Layout,
    halo_overlay: Option<usize>,
    hold: Option<Hold>,
    pending_move: Option<Point>,
    last_tick: Option<Instant>,
}

impl Pipeline {
    pub(crate) fn new(os: Os, config: Config, layout: Layout) -> Self {
        Self {
            os,
            config,
            layout,
            halo_overlay: None,
            hold: None,
            pending_move: None,
            last_tick: None,
        }
    }

    pub(crate) fn handle(&mut self, msg: Msg, now: Instant) -> Vec<Emit> {
        match msg {
            Msg::Config(config) => {
                self.config = config;
                let mut emits = Vec::new();
                if !config.enabled {
                    emits.extend(self.end_hold());
                }
                if !(config.enabled && config.halo) {
                    self.pending_move = None;
                    emits.extend(self.clear_halo());
                }
                emits
            }
            Msg::Layout(layout) => {
                self.layout = layout;
                self.pending_move = None;
                let mut emits = self.clear_halo();
                if let Some(hold) = &mut self.hold {
                    emits.extend(follow(
                        &mut hold.overlay,
                        None,
                        OverlayEvent::Hold { hold: None },
                    ));
                }
                emits
            }
            Msg::Input(_) if !self.config.enabled => vec![],
            Msg::Input(InputEvent::Press { button, mods, at }) => {
                // A move still waiting for its tick is older than the press.
                self.pending_move = self.pending_move.map(|_| at);
                let mut emits = match &self.hold {
                    Some(hold) if hold.button == button => self.end_hold(),
                    _ => vec![],
                };
                emits.extend(self.layout.locate(at).map(|(overlay, css)| Emit {
                    overlay,
                    event: OverlayEvent::Click {
                        button,
                        mods: mods.label(self.os),
                        x: css.x,
                        y: css.y,
                    },
                }));
                if self.hold.is_none() {
                    self.hold = Some(Hold {
                        button,
                        mods,
                        overlay: None,
                    });
                    emits.extend(self.place_hold(at));
                }
                emits
            }
            Msg::Input(InputEvent::Release { button, at }) => match &self.hold {
                Some(hold) if hold.button == button => self.release(at),
                _ => vec![],
            },
            Msg::Input(InputEvent::Move { at, buttons_down }) => {
                let mut emits = if buttons_down {
                    vec![]
                } else {
                    self.release(at)
                };
                if self.config.halo || self.hold.is_some() {
                    self.pending_move = Some(at);
                    match self.deadline() {
                        Some(deadline) if now < deadline => {}
                        _ => emits.extend(self.flush(now)),
                    }
                }
                emits
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
        self.pending_move
            .and(self.last_tick)
            .map(|last| last + TICK)
    }

    /// One tick moves the halo and the held button together.
    pub(crate) fn flush(&mut self, now: Instant) -> Vec<Emit> {
        let Some(at) = self.pending_move.take() else {
            return vec![];
        };
        self.last_tick = Some(now);
        let mut emits = Vec::new();
        if self.config.halo {
            let located = self.layout.locate(at);
            emits.extend(follow(
                &mut self.halo_overlay,
                located.map(|(overlay, css)| (overlay, OverlayEvent::Halo { at: Some(css) })),
                OverlayEvent::Halo { at: None },
            ));
        }
        emits.extend(self.place_hold(at));
        emits
    }

    fn place_hold(&mut self, at: Point) -> Vec<Emit> {
        let Some(hold) = &mut self.hold else {
            return vec![];
        };
        let view = |(overlay, css): (usize, Point)| {
            let view = HoldView {
                button: hold.button,
                mods: hold.mods.label(self.os),
                x: css.x,
                y: css.y,
            };
            (overlay, OverlayEvent::Hold { hold: Some(view) })
        };
        let shown = self.layout.locate(at).map(view);
        follow(&mut hold.overlay, shown, OverlayEvent::Hold { hold: None })
    }

    /// Ends the hold at its final point, so the drawn path reaches where the button came up.
    fn release(&mut self, at: Point) -> Vec<Emit> {
        let mut emits = self.place_hold(at);
        emits.extend(self.end_hold());
        emits
    }

    fn end_hold(&mut self) -> Vec<Emit> {
        self.hold.take().map_or(vec![], |mut hold| {
            follow(&mut hold.overlay, None, OverlayEvent::Hold { hold: None })
        })
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
                if let Err(error) = app.emit_to(Surface::Overlay(overlay).label(), "overlay", event)
                {
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
        input(InputEvent::Move {
            at: at(x, y),
            buttons_down: false,
        })
    }
    fn drag(x: f64, y: f64) -> Msg {
        input(InputEvent::Move {
            at: at(x, y),
            buttons_down: true,
        })
    }
    fn press(button: Button, mods: Mods, x: f64, y: f64) -> Msg {
        input(InputEvent::Press {
            button,
            mods,
            at: at(x, y),
        })
    }
    fn release(button: Button, x: f64, y: f64) -> Msg {
        input(InputEvent::Release {
            button,
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
    fn held(overlay: usize, button: Button, mods: Option<&str>, x: f64, y: f64) -> Emit {
        Emit {
            overlay,
            event: OverlayEvent::Hold {
                hold: Some(HoldView {
                    button,
                    mods: mods.map(Into::into),
                    x,
                    y,
                }),
            },
        }
    }
    fn unheld(overlay: usize) -> Emit {
        Emit {
            overlay,
            event: OverlayEvent::Hold { hold: None },
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
            [
                click(1, Button::Right, None, 250.0, 200.0),
                held(1, Button::Right, None, 250.0, 200.0)
            ]
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
    fn a_press_ripples_with_its_modifiers_and_holds_until_its_release() {
        let now = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        let shift_command = Mods::SHIFT | Mods::META;
        assert_eq!(
            pipeline.handle(press(Button::Left, shift_command, 100.0, 50.0), now),
            [
                click(0, Button::Left, Some("⇧⌘"), 100.0, 50.0),
                held(0, Button::Left, Some("⇧⌘"), 100.0, 50.0)
            ]
        );
        assert_eq!(
            pipeline.handle(release(Button::Left, 104.0, 50.0), now),
            [held(0, Button::Left, Some("⇧⌘"), 104.0, 50.0), unheld(0)],
            "the path ends where the button came up"
        );
        assert_eq!(pipeline.handle(release(Button::Left, 104.0, 50.0), now), []);
        let mut windows = Pipeline::new(Os::Windows, HALO_OFF, two_displays());
        assert_eq!(
            windows.handle(press(Button::Middle, Mods::CTRL, 10.0, 10.0), now)[0],
            click(0, Button::Middle, Some("Ctrl"), 10.0, 10.0)
        );
    }

    #[test]
    fn a_drag_with_the_halo_off_moves_the_hold_once_per_tick() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), start);
        let mut emitted = Vec::new();
        for ms in 1..=40u32 {
            let now = start + Duration::from_millis(u64::from(ms));
            emitted.extend(pipeline.handle(drag(10.0 + f64::from(ms), 10.0), now));
        }
        assert_eq!(
            emitted,
            [
                held(0, Button::Left, None, 11.0, 10.0),
                held(0, Button::Left, None, 27.0, 10.0),
                held(0, Button::Left, None, 43.0, 10.0)
            ]
        );
        assert_eq!(
            pipeline.flush(start + Duration::from_millis(49)),
            [held(0, Button::Left, None, 50.0, 10.0)]
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
            [
                halo(0, Some(at(30.0, 10.0))),
                held(0, Button::Left, None, 30.0, 10.0)
            ],
            "the path never runs back to where the pointer was before the press"
        );
    }

    #[test]
    fn a_drag_with_the_halo_on_moves_both_in_one_tick() {
        let now = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), now);
        assert_eq!(
            pipeline.handle(drag(20.0, 10.0), now),
            [
                halo(0, Some(at(20.0, 10.0))),
                held(0, Button::Left, None, 20.0, 10.0)
            ]
        );
    }

    #[test]
    fn a_drag_across_displays_clears_the_overlay_it_leaves_first() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 990.0, 10.0), start);
        let mut tick = 0;
        let mut drag_to = |x, y| {
            tick += 1;
            pipeline.handle(drag(x, y), start + TICK * tick)
        };
        assert_eq!(
            drag_to(1010.0, 10.0),
            [unheld(0), held(1, Button::Left, None, 5.0, 5.0)]
        );
        assert_eq!(drag_to(1010.0, 900.0), [unheld(1)], "below every display");
        assert_eq!(
            drag_to(500.0, 10.0),
            [held(0, Button::Left, None, 500.0, 10.0)]
        );
    }

    #[test]
    fn one_button_is_held_at_a_time() {
        let now = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), now);
        assert_eq!(
            pipeline.handle(press(Button::Right, Mods::NONE, 10.0, 10.0), now),
            [click(0, Button::Right, None, 10.0, 10.0)],
            "a second button only ripples"
        );
        assert_eq!(pipeline.handle(release(Button::Right, 10.0, 10.0), now), []);
        assert_eq!(
            pipeline.handle(press(Button::Left, Mods::NONE, 20.0, 10.0), now),
            [
                unheld(0),
                click(0, Button::Left, None, 20.0, 10.0),
                held(0, Button::Left, None, 20.0, 10.0)
            ],
            "a press of the held button means its release was missed"
        );
    }

    #[test]
    fn motion_with_no_button_down_ends_a_hold_whose_release_was_missed() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), start);
        assert_eq!(
            pipeline.handle(hover(30.0, 10.0), start),
            [held(0, Button::Left, None, 30.0, 10.0), unheld(0)]
        );
        assert_eq!(pipeline.handle(hover(40.0, 10.0), start + TICK), []);
        assert_eq!(pipeline.deadline(), None, "nothing left to move");

        let mut halo_on = Pipeline::new(Os::Mac, ON, two_displays());
        halo_on.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), start);
        assert_eq!(
            halo_on.handle(hover(30.0, 10.0), start),
            [
                held(0, Button::Left, None, 30.0, 10.0),
                unheld(0),
                halo(0, Some(at(30.0, 10.0)))
            ]
        );
    }

    #[test]
    fn a_layout_change_hides_the_hold_until_it_moves() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), start);
        assert_eq!(
            pipeline.handle(Msg::Layout(two_displays()), start),
            [unheld(0)]
        );
        assert_eq!(
            pipeline.handle(drag(20.0, 10.0), start + TICK),
            [held(0, Button::Left, None, 20.0, 10.0)]
        );
    }

    #[test]
    fn disabling_ends_the_hold_and_forgets_its_button() {
        let now = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, HALO_OFF, two_displays());
        pipeline.handle(press(Button::Left, Mods::NONE, 10.0, 10.0), now);
        let disabled = Config {
            enabled: false,
            halo: false,
        };
        assert_eq!(pipeline.handle(Msg::Config(disabled), now), [unheld(0)]);
        pipeline.handle(Msg::Config(HALO_OFF), now);
        assert_eq!(pipeline.handle(release(Button::Left, 10.0, 10.0), now), []);
    }

    #[test]
    fn other_mac_buttons_decode_by_number() {
        assert_eq!(Button::from_mac_number(2), Some(Button::Middle));
        assert_eq!(Button::from_mac_number(5), None);
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
            json(held(0, Button::Left, None, 3.0, 4.0).event),
            serde_json::json!({"kind": "hold", "hold": {"button": "left", "mods": null, "x": 3.0, "y": 4.0}})
        );
        assert_eq!(
            json(unheld(0).event),
            serde_json::json!({"kind": "hold", "hold": null})
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
