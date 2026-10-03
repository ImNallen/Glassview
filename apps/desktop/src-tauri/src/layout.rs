#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub(crate) struct Point {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}
impl Rect {
    fn contains(&self, at: Point) -> bool {
        at.x >= self.x
            && at.y >= self.y
            && at.x < self.x + self.width
            && at.y < self.y + self.height
    }
}

/// The space native input arrives in, which is also the space overlay windows are placed in:
/// macOS points from the primary display's top-left (Tauri Logical), Windows physical
/// virtual-screen pixels (Tauri Physical).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InputSpace {
    Points,
    Physical,
}
impl InputSpace {
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Points
    } else {
        Self::Physical
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MonitorInfo {
    pub(crate) position: (i32, i32),
    pub(crate) size: (u32, u32),
    pub(crate) scale: f64,
}
impl From<&tauri::Monitor> for MonitorInfo {
    fn from(monitor: &tauri::Monitor) -> Self {
        Self {
            position: (monitor.position().x, monitor.position().y),
            size: (monitor.size().width, monitor.size().height),
            scale: monitor.scale_factor(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Display {
    pub(crate) bounds: Rect,
    pub(crate) css_per_unit: f64,
}

/// Every display in input space, sorted by position so overlay indices stay put
/// while the OS reorders its monitor list.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Layout(Vec<Display>);

impl Layout {
    pub(crate) fn from_monitors(monitors: &[MonitorInfo], space: InputSpace) -> Self {
        let mut displays: Vec<_> = monitors
            .iter()
            .map(|m| {
                let (x, y) = (f64::from(m.position.0), f64::from(m.position.1));
                let (width, height) = (f64::from(m.size.0), f64::from(m.size.1));
                match space {
                    // tao reports each macOS monitor as points times that monitor's own
                    // scale, so dividing by it recovers CGDisplayBounds exactly.
                    InputSpace::Points => Display {
                        bounds: Rect {
                            x: x / m.scale,
                            y: y / m.scale,
                            width: width / m.scale,
                            height: height / m.scale,
                        },
                        css_per_unit: 1.0,
                    },
                    InputSpace::Physical => Display {
                        bounds: Rect {
                            x,
                            y,
                            width,
                            height,
                        },
                        css_per_unit: 1.0 / m.scale,
                    },
                }
            })
            .collect();
        displays.sort_by(|a, b| {
            (a.bounds.x, a.bounds.y)
                .partial_cmp(&(b.bounds.x, b.bounds.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self(displays)
    }
    pub(crate) fn displays(&self) -> &[Display] {
        &self.0
    }
    pub(crate) fn locate(&self, at: Point) -> Option<(usize, Point)> {
        self.0.iter().enumerate().find_map(|(index, display)| {
            let Display {
                bounds,
                css_per_unit: k,
            } = display;
            bounds.contains(at).then(|| {
                let css = Point {
                    x: (at.x - bounds.x) * k,
                    y: (at.y - bounds.y) * k,
                };
                (index, css)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(position: (i32, i32), size: (u32, u32), scale: f64) -> MonitorInfo {
        MonitorInfo {
            position,
            size,
            scale,
        }
    }
    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    fn at(x: f64, y: f64) -> Point {
        Point { x, y }
    }
    fn bounds(layout: &Layout) -> Vec<Rect> {
        layout.displays().iter().map(|d| d.bounds).collect()
    }

    #[test]
    fn mac_ultrawide_1x_primary_with_a_2x_display_to_its_right() {
        let layout = Layout::from_monitors(
            &[
                monitor((0, 0), (5120, 1440), 1.0),
                monitor((10240, 0), (3840, 2160), 2.0),
            ],
            InputSpace::Points,
        );
        assert_eq!(
            bounds(&layout),
            [
                rect(0.0, 0.0, 5120.0, 1440.0),
                rect(5120.0, 0.0, 1920.0, 1080.0)
            ]
        );
        assert_eq!(
            layout.locate(at(5119.5, 1439.0)),
            Some((0, at(5119.5, 1439.0)))
        );
        assert_eq!(layout.locate(at(5120.0, 0.0)), Some((1, at(0.0, 0.0))));
        assert_eq!(
            layout.locate(at(7000.0, 1000.0)),
            Some((1, at(1880.0, 1000.0)))
        );
        assert_eq!(
            layout.locate(at(6000.0, 1200.0)),
            None,
            "below the shorter display"
        );
    }

    #[test]
    fn mac_retina_primary_with_a_1x_display_to_its_right() {
        let layout = Layout::from_monitors(
            &[
                monitor((0, 0), (2880, 1800), 2.0),
                monitor((1440, 0), (1920, 1080), 1.0),
            ],
            InputSpace::Points,
        );
        assert_eq!(
            bounds(&layout),
            [
                rect(0.0, 0.0, 1440.0, 900.0),
                rect(1440.0, 0.0, 1920.0, 1080.0)
            ]
        );
        assert_eq!(layout.locate(at(1500.0, 10.0)), Some((1, at(60.0, 10.0))));
        assert_eq!(layout.locate(at(720.0, 450.0)), Some((0, at(720.0, 450.0))));
    }

    #[test]
    fn mac_display_left_of_and_above_the_primary_sorts_first() {
        let layout = Layout::from_monitors(
            &[
                monitor((0, 0), (1440, 900), 1.0),
                monitor((-1920, -200), (1920, 1080), 1.0),
            ],
            InputSpace::Points,
        );
        assert_eq!(
            bounds(&layout),
            [
                rect(-1920.0, -200.0, 1920.0, 1080.0),
                rect(0.0, 0.0, 1440.0, 900.0)
            ]
        );
        assert_eq!(layout.locate(at(-1.0, 0.0)), Some((0, at(1919.0, 200.0))));
        assert_eq!(layout.locate(at(0.0, 0.0)), Some((1, at(0.0, 0.0))));
    }

    #[test]
    fn windows_150_percent_primary_with_a_100_percent_display_to_its_right() {
        let layout = Layout::from_monitors(
            &[
                monitor((3840, 0), (1920, 1080), 1.0),
                monitor((0, 0), (3840, 2160), 1.5),
            ],
            InputSpace::Physical,
        );
        assert_eq!(
            bounds(&layout),
            [
                rect(0.0, 0.0, 3840.0, 2160.0),
                rect(3840.0, 0.0, 1920.0, 1080.0)
            ]
        );
        assert_eq!(
            layout.locate(at(3000.0, 1500.0)),
            Some((0, at(2000.0, 1000.0)))
        );
        assert_eq!(layout.locate(at(3840.0, 0.0)), Some((1, at(0.0, 0.0))));
        assert_eq!(layout.locate(at(4000.0, 1500.0)), None);
    }

    #[test]
    fn windows_125_percent_display_at_a_negative_origin() {
        let layout = Layout::from_monitors(
            &[
                monitor((0, 0), (1920, 1080), 1.0),
                monitor((-2560, -360), (2560, 1440), 1.25),
            ],
            InputSpace::Physical,
        );
        assert_eq!(layout.locate(at(-2555.0, -355.0)), Some((0, at(4.0, 4.0))));
        assert_eq!(
            layout.locate(at(-5.0, 1075.0)),
            Some((0, at(2044.0, 1148.0)))
        );
        assert_eq!(layout.locate(at(100.0, 100.0)), Some((1, at(100.0, 100.0))));
    }
}
