use crate::{
    keys::{Os, Stroke, StrokeView},
    layout::{Layout, Point},
    overlays::Surface,
};
use serde::Serialize;
use std::{
    sync::mpsc::{self, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};
use tauri::Emitter;

const HALO_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Button {
    Left,
    Right,
    Middle,
}

pub(crate) enum InputEvent {
    Click { button: Button, at: Point },
    Move { at: Point },
    Key { stroke: Stroke, cursor: Point },
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
    Click { button: Button, x: f64, y: f64 },
    Halo { at: Option<Point> },
    Key { stroke: StrokeView },
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
    last_halo: Option<Instant>,
}

impl Pipeline {
    pub(crate) fn new(os: Os, config: Config, layout: Layout) -> Self {
        Self {
            os,
            config,
            layout,
            halo_overlay: None,
            pending_move: None,
            last_halo: None,
        }
    }

    pub(crate) fn handle(&mut self, msg: Msg, now: Instant) -> Vec<Emit> {
        match msg {
            Msg::Config(config) => {
                self.config = config;
                if config.enabled && config.halo {
                    return vec![];
                }
                self.pending_move = None;
                self.clear_halo().into_iter().collect()
            }
            Msg::Layout(layout) => {
                self.layout = layout;
                self.pending_move = None;
                self.clear_halo().into_iter().collect()
            }
            Msg::Input(_) if !self.config.enabled => vec![],
            Msg::Input(InputEvent::Click { button, at }) => self
                .layout
                .locate(at)
                .map(|(overlay, css)| Emit {
                    overlay,
                    event: OverlayEvent::Click {
                        button,
                        x: css.x,
                        y: css.y,
                    },
                })
                .into_iter()
                .collect(),
            Msg::Input(InputEvent::Move { at }) => {
                if !self.config.halo {
                    return vec![];
                }
                self.pending_move = Some(at);
                match self.deadline() {
                    Some(deadline) if now < deadline => vec![],
                    _ => self.flush(now),
                }
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
            .and(self.last_halo)
            .map(|last| last + HALO_INTERVAL)
    }

    pub(crate) fn flush(&mut self, now: Instant) -> Vec<Emit> {
        let Some(at) = self.pending_move.take() else {
            return vec![];
        };
        self.last_halo = Some(now);
        let Some((overlay, css)) = self.layout.locate(at) else {
            return self.clear_halo().into_iter().collect();
        };
        let left = self.halo_overlay.filter(|&old| old != overlay);
        self.halo_overlay = Some(overlay);
        left.map(|old| Emit {
            overlay: old,
            event: OverlayEvent::Halo { at: None },
        })
        .into_iter()
        .chain([Emit {
            overlay,
            event: OverlayEvent::Halo { at: Some(css) },
        }])
        .collect()
    }

    fn clear_halo(&mut self) -> Option<Emit> {
        self.halo_overlay.take().map(|overlay| Emit {
            overlay,
            event: OverlayEvent::Halo { at: None },
        })
    }
}

#[derive(Clone)]
pub(crate) struct PipelineHandle(Sender<Msg>);
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
            emitted.extend(pipeline.handle(input(InputEvent::Move { at: at(x, 10.0) }), now));
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
            pipeline.handle(
                input(InputEvent::Move {
                    at: at(990.0, 10.0)
                }),
                start
            ),
            [halo(0, Some(at(990.0, 10.0)))]
        );
        assert_eq!(
            pipeline.handle(
                input(InputEvent::Move {
                    at: at(1010.0, 10.0)
                }),
                start + HALO_INTERVAL
            ),
            [halo(0, None), halo(1, Some(at(5.0, 5.0)))]
        );
        let off = Config {
            enabled: true,
            halo: false,
        };
        assert_eq!(pipeline.handle(Msg::Config(off), start), [halo(1, None)]);
        let later = start + Duration::from_secs(1);
        assert_eq!(
            pipeline.handle(input(InputEvent::Move { at: at(10.0, 10.0) }), later),
            []
        );
    }

    #[test]
    fn disabling_clears_the_halo_and_drops_input_in_flight() {
        let start = Instant::now();
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        pipeline.handle(input(InputEvent::Move { at: at(1.0, 1.0) }), start);
        let disabled = Config {
            enabled: false,
            halo: true,
        };
        assert_eq!(
            pipeline.handle(Msg::Config(disabled), start),
            [halo(0, None)]
        );
        let click = InputEvent::Click {
            button: Button::Left,
            at: at(5.0, 5.0),
        };
        assert_eq!(pipeline.handle(input(click), start), []);
        let key = InputEvent::Key {
            stroke: command_p(),
            cursor: at(5.0, 5.0),
        };
        assert_eq!(pipeline.handle(input(key), start), []);
    }

    #[test]
    fn clicks_land_in_the_css_pixels_of_their_display() {
        let mut pipeline = Pipeline::new(Os::Mac, ON, two_displays());
        let click = |button, x, y| {
            input(InputEvent::Click {
                button,
                at: at(x, y),
            })
        };
        assert_eq!(
            pipeline.handle(click(Button::Right, 1500.0, 400.0), Instant::now()),
            [Emit {
                overlay: 1,
                event: OverlayEvent::Click {
                    button: Button::Right,
                    x: 250.0,
                    y: 200.0
                }
            }]
        );
        assert_eq!(
            pipeline.handle(click(Button::Left, 500.0, 900.0), Instant::now()),
            [],
            "below every display"
        );
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
                x: 1.5,
                y: 2.0
            }),
            serde_json::json!({"kind": "click", "button": "middle", "x": 1.5, "y": 2.0})
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
