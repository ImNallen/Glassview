use tauri::Manager;

/// A webview window. The label names it to Tauri and `?surface=` tells the frontend what to render.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Surface {
    /// The overlay covering one display, by `Layout` index.
    Overlay(usize),
    Settings,
}
impl Surface {
    pub(crate) fn url(self) -> &'static str {
        match self {
            Self::Overlay(_) => "overlay",
            Self::Settings => "settings",
        }
    }
    pub(crate) fn label(self) -> String {
        match self {
            Self::Overlay(index) => format!("overlay-{index}"),
            Self::Settings => "settings".into(),
        }
    }
    pub(crate) fn parse(label: &str) -> Option<Self> {
        match label.strip_prefix("overlay-") {
            Some(index) => index.parse().ok().map(Self::Overlay),
            None => (label == "settings").then_some(Self::Settings),
        }
    }
    pub(crate) fn window<R: tauri::Runtime>(
        self,
        app: &impl Manager<R>,
    ) -> Option<tauri::WebviewWindow<R>> {
        app.get_webview_window(&self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::Surface::{self, *};
    #[test]
    fn surfaces_keep_the_labels_and_urls_the_frontend_reads() {
        for (surface, label, url) in [
            (Overlay(2), "overlay-2", "overlay"),
            (Settings, "settings", "settings"),
        ] {
            assert_eq!((surface.label().as_str(), surface.url()), (label, url));
            assert_eq!(Surface::parse(label), Some(surface));
        }
        for label in ["overlay-", "overlay-x", "main"] {
            assert_eq!(Surface::parse(label), None);
        }
    }
}
