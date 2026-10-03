use serde::{Deserialize, Serialize};

/// Held modifiers. META is ⌘ on macOS and Win on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Mods(u8);
impl Mods {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const CTRL: Self = Self(1);
    pub(crate) const ALT: Self = Self(2);
    pub(crate) const SHIFT: Self = Self(4);
    pub(crate) const META: Self = Self(8);
    pub(crate) fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    /// Ctrl, Alt/Option, or Cmd/Win: the modifiers that can make a press a shortcut.
    pub(crate) fn commands(self) -> bool {
        self.0 & (Self::CTRL.0 | Self::ALT.0 | Self::META.0) != 0
    }
}
impl std::ops::BitOr for Mods {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    Enter,
    Tab,
    Space,
    Backspace,
    Delete,
    Escape,
    Insert,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// What a key is called on its keycap. `Char` is the layout's unshifted character, uppercased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    Named(Named),
}

/// A key press as a platform layer observed it. Modifier-only presses never become one.
/// No `Debug`, so typed text cannot reach a log line by accident.
pub(crate) struct KeyPress {
    pub(crate) mods: Mods,
    pub(crate) key: Key,
    /// The character this press types with its current modifiers, if any.
    pub(crate) text: Option<char>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum KeyMode {
    Shortcuts,
    All,
}

/// A press the privacy filter allowed to be shown. Only `admit` constructs one.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub(crate) struct Stroke {
    mods: Mods,
    key: Key,
    text: Option<char>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Mac,
    Windows,
}
impl Os {
    pub(crate) const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else {
        Self::Windows
    };
}

/// What an overlay renders for one stroke. The webview never sees modifiers or key codes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum StrokeView {
    Chord { label: String },
    Text { text: String },
}

/// Runs before any key translation: in Shortcuts mode a press with no command
/// modifier can never be shown, so it is dropped untranslated.
pub(crate) fn may_show(mods: Mods, mode: KeyMode) -> bool {
    mode == KeyMode::All || mods.commands()
}

/// The privacy filter. Shortcuts mode shows a press only when a command
/// modifier is held and the press types nothing.
pub(crate) fn admit(press: KeyPress, mode: KeyMode) -> Option<Stroke> {
    let KeyPress { mods, key, text } = press;
    let shown = match mode {
        KeyMode::All => true,
        KeyMode::Shortcuts => mods.commands() && text.is_none(),
    };
    shown.then_some(Stroke { mods, key, text })
}

fn printable(c: &char) -> bool {
    !c.is_control()
}

/// macOS: ⌘ or ⌃ turn any key into a command. Otherwise, including with ⌥ and ⇧,
/// the layout decides, so ⌥2 = "™" is text and ⌥← is not.
// Each OS rule is used by one platform layer but tested on every host.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn mac_typed_text(mods: Mods, translate: impl FnOnce() -> Option<char>) -> Option<char> {
    if mods.has(Mods::META) || mods.has(Mods::CTRL) {
        return None;
    }
    translate().filter(printable)
}

/// Windows: AltGr arrives as Ctrl+Alt, and Alt with numpad digits types Alt codes.
/// `numpad_digit` is the digit a numpad key stands for, decided by key identity
/// because translating with Alt held yields nothing for Alt codes.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_typed_text(
    mods: Mods,
    numpad_digit: Option<char>,
    translate: impl FnOnce() -> Option<char>,
) -> Option<char> {
    let (ctrl, alt) = (mods.has(Mods::CTRL), mods.has(Mods::ALT));
    if mods.has(Mods::META) {
        None
    } else if alt && !ctrl {
        numpad_digit
    } else if ctrl && !alt {
        None
    } else {
        translate().filter(printable)
    }
}

impl Stroke {
    pub(crate) fn view(&self, os: Os) -> StrokeView {
        match self.text {
            Some(c) => StrokeView::Text { text: c.into() },
            None => StrokeView::Chord {
                label: self.label(os),
            },
        }
    }
    /// macOS: "⇧⌘P" in ⌃⌥⇧⌘ order. Windows: "Ctrl+Alt+Del" in Win, Ctrl, Alt, Shift order.
    fn label(&self, os: Os) -> String {
        let key = key_name(self.key, os);
        match os {
            Os::Mac => [
                (Mods::CTRL, "⌃"),
                (Mods::ALT, "⌥"),
                (Mods::SHIFT, "⇧"),
                (Mods::META, "⌘"),
            ]
            .into_iter()
            .filter(|(m, _)| self.mods.has(*m))
            .map(|(_, glyph)| glyph)
            .chain([key.as_str()])
            .collect(),
            Os::Windows => [
                (Mods::META, "Win"),
                (Mods::CTRL, "Ctrl"),
                (Mods::ALT, "Alt"),
                (Mods::SHIFT, "Shift"),
            ]
            .into_iter()
            .filter(|(m, _)| self.mods.has(*m))
            .map(|(_, name)| name)
            .chain([key.as_str()])
            .collect::<Vec<_>>()
            .join("+"),
        }
    }
}

fn key_name(key: Key, os: Os) -> String {
    let named = match key {
        Key::Char(c) => return c.to_uppercase().collect(),
        Key::Named(named) => named,
    };
    let (mac, windows) = match named {
        Named::F(n) => return format!("F{n}"),
        Named::Enter => ("↩", "Enter"),
        Named::Tab => ("⇥", "Tab"),
        Named::Space => ("␣", "Space"),
        Named::Backspace => ("⌫", "Backspace"),
        Named::Delete => ("⌦", "Del"),
        Named::Escape => ("⎋", "Esc"),
        Named::Insert => ("Ins", "Ins"),
        Named::Left => ("←", "←"),
        Named::Right => ("→", "→"),
        Named::Up => ("↑", "↑"),
        Named::Down => ("↓", "↓"),
        Named::Home => ("↖", "Home"),
        Named::End => ("↘", "End"),
        Named::PageUp => ("⇞", "PgUp"),
        Named::PageDown => ("⇟", "PgDn"),
    };
    match os {
        Os::Mac => mac,
        Os::Windows => windows,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::{Key::*, KeyMode::*, Mods as M, Named::*, *};

    fn chord(label: &str) -> Option<StrokeView> {
        Some(StrokeView::Chord {
            label: label.into(),
        })
    }
    fn text(text: &str) -> Option<StrokeView> {
        Some(StrokeView::Text { text: text.into() })
    }
    /// Runs a press through the same steps a platform layer does: the
    /// pre-check, the OS rule for typed text, then the filter.
    fn shown(
        os: Os,
        mode: KeyMode,
        mods: M,
        key: Key,
        numpad_digit: Option<char>,
        layout: Option<char>,
    ) -> Option<StrokeView> {
        if !may_show(mods, mode) {
            return None;
        }
        let text = match os {
            Os::Mac => mac_typed_text(mods, || layout),
            Os::Windows => windows_typed_text(mods, numpad_digit, || layout),
        };
        admit(KeyPress { mods, key, text }, mode).map(|stroke| stroke.view(os))
    }

    #[test]
    fn mac_shows_commands_and_hides_typing_in_shortcuts_mode() {
        let mac = |mods, key, layout| shown(Os::Mac, Shortcuts, mods, key, None, layout);
        assert_eq!(mac(M::META, Char('p'), Some('p')), chord("⌘P"));
        assert_eq!(mac(M::SHIFT | M::META, Char('p'), Some('P')), chord("⇧⌘P"));
        assert_eq!(
            mac(M::CTRL | M::ALT | M::SHIFT | M::META, Char('k'), Some('K')),
            chord("⌃⌥⇧⌘K")
        );
        assert_eq!(mac(M::CTRL, Char('c'), Some('\u{3}')), chord("⌃C"));
        assert_eq!(mac(M::NONE, Char('p'), Some('p')), None);
        assert_eq!(mac(M::SHIFT, Char('p'), Some('P')), None);
        assert_eq!(mac(M::SHIFT, Named(Tab), Some('\t')), None);
        assert_eq!(mac(M::NONE, Named(Escape), None), None);
        assert_eq!(mac(M::NONE, Named(F(5)), None), None);
        assert_eq!(mac(M::ALT, Char('2'), Some('™')), None, "⌥2 types ™");
        assert_eq!(mac(M::ALT, Char('e'), None), chord("⌥E"), "dead key");
        assert_eq!(mac(M::ALT, Named(Left), None), chord("⌥←"));
        assert_eq!(mac(M::META, Named(Backspace), None), chord("⌘⌫"));
        assert_eq!(mac(M::META, Named(Space), Some(' ')), chord("⌘␣"));
        assert_eq!(mac(M::CTRL | M::SHIFT, Named(F(12)), None), chord("⌃⇧F12"));
    }

    #[test]
    fn mac_shows_everything_in_all_keys_mode() {
        let mac = |mods, key, layout| shown(Os::Mac, All, mods, key, None, layout);
        assert_eq!(mac(M::NONE, Char('h'), Some('h')), text("h"));
        assert_eq!(mac(M::SHIFT, Char('h'), Some('H')), text("H"));
        assert_eq!(mac(M::ALT, Char('2'), Some('™')), text("™"));
        assert_eq!(mac(M::NONE, Named(Space), Some(' ')), text(" "));
        assert_eq!(mac(M::NONE, Named(Enter), Some('\r')), chord("↩"));
        assert_eq!(mac(M::NONE, Named(Escape), None), chord("⎋"));
        assert_eq!(mac(M::META, Char('z'), Some('z')), chord("⌘Z"));
    }

    #[test]
    fn windows_shows_commands_and_hides_typing_in_shortcuts_mode() {
        let win =
            |mods, key, digit, layout| shown(Os::Windows, Shortcuts, mods, key, digit, layout);
        assert_eq!(
            win(M::CTRL, Char('c'), None, Some('\u{3}')),
            chord("Ctrl+C")
        );
        assert_eq!(win(M::ALT, Char('f'), None, None), chord("Alt+F"));
        assert_eq!(win(M::META, Char('d'), None, Some('d')), chord("Win+D"));
        assert_eq!(
            win(M::META | M::SHIFT, Char('s'), None, Some('S')),
            chord("Win+Shift+S")
        );
        assert_eq!(
            win(M::CTRL | M::ALT, Named(Delete), None, None),
            chord("Ctrl+Alt+Del")
        );
        assert_eq!(
            win(M::CTRL | M::SHIFT, Char('p'), None, None),
            chord("Ctrl+Shift+P")
        );
        assert_eq!(
            win(M::CTRL | M::ALT, Char('q'), None, Some('@')),
            None,
            "AltGr"
        );
        assert_eq!(
            win(M::CTRL | M::ALT, Char('t'), None, None),
            chord("Ctrl+Alt+T")
        );
        assert_eq!(
            win(M::ALT, Char('1'), Some('1'), None),
            None,
            "Alt code digit"
        );
        assert_eq!(win(M::ALT | M::SHIFT, Char('1'), Some('1'), None), None);
        assert_eq!(win(M::ALT, Named(Tab), None, Some('\t')), chord("Alt+Tab"));
        assert_eq!(win(M::NONE, Char('a'), None, Some('a')), None);
        assert_eq!(win(M::SHIFT, Char('a'), None, Some('A')), None);
        assert_eq!(win(M::NONE, Named(F(5)), None, None), None);
        assert_eq!(
            win(M::CTRL, Named(PageDown), None, None),
            chord("Ctrl+PgDn")
        );
    }

    #[test]
    fn windows_shows_everything_in_all_keys_mode() {
        let win = |mods, key, digit, layout| shown(Os::Windows, All, mods, key, digit, layout);
        assert_eq!(win(M::NONE, Char('h'), None, Some('h')), text("h"));
        assert_eq!(win(M::CTRL | M::ALT, Char('q'), None, Some('@')), text("@"));
        assert_eq!(win(M::ALT, Char('1'), Some('1'), None), text("1"));
        assert_eq!(
            win(M::NONE, Named(Backspace), None, Some('\u{8}')),
            chord("Backspace")
        );
        assert_eq!(
            win(M::NONE, Named(Escape), None, Some('\u{1b}')),
            chord("Esc")
        );
    }

    #[test]
    fn plain_and_shifted_presses_are_dropped_before_translation_in_shortcuts_mode() {
        for mods in [M::NONE, M::SHIFT] {
            assert!(!may_show(mods, Shortcuts));
            assert!(may_show(mods, All));
        }
        for mods in [M::CTRL, M::ALT, M::META, M::SHIFT | M::META] {
            assert!(may_show(mods, Shortcuts));
        }
        let mut translated = false;
        assert_eq!(
            mac_typed_text(M::META, || {
                translated = true;
                Some('p')
            }),
            None
        );
        assert!(!translated, "⌘ decides without asking the layout");
    }
}
