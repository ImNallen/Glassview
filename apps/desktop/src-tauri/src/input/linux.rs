use super::Sink;
use crate::{
    keys::{self, Key, KeyPress, Mods, Named},
    layout::Point,
    pipeline::{Button, ScrollDelta},
    Result,
};
use std::sync::mpsc;
use x11rb::{
    connection::Connection,
    protocol::{
        xinput::{self, Device, XIEventMask},
        xkb::{self, ConnectionExt as _, GetStateReply},
        xproto::{ConnectionExt as _, Keycode, Keysym, Mapping, ModMask, Window},
        Event,
    },
    rust_connection::RustConnection,
};
use xkeysym::key;

type Fallible<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const NO_SYMBOL: Keysym = 0;

/// XInput2 raw events on the root window report every key and button without a
/// grab, but carry no position or modifier state, so each one is followed by a
/// query to the server.
pub(super) fn start(sink: Sink) -> Result<()> {
    if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
        return Err("On Linux, Glassview needs an X11 session.".into());
    }
    let (ready, connected) = mpsc::channel();
    std::thread::Builder::new()
        .name("glassview-input".into())
        .spawn(move || {
            let watcher = match Watcher::connect() {
                Ok(watcher) => watcher,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            let _ = ready.send(Ok(()));
            watcher.run(&sink);
        })?;
    connected
        .recv()?
        .map_err(|error| format!("Could not watch the keyboard and mouse: {error}"))?;
    log::info!("Watching the keyboard and mouse through XInput2");
    Ok(())
}

struct Watcher {
    conn: RustConnection,
    root: Window,
    keymap: Keymap,
}
impl Watcher {
    fn connect() -> Fallible<Self> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        // From XI 2.1 the server also sends raw events for master devices, so
        // selecting on them gets each press once however many devices feed it.
        let version = xinput::xi_query_version(&conn, 2, 2)?.reply()?;
        if (version.major_version, version.minor_version) < (2, 1) {
            return Err(format!(
                "XInput {}.{} is too old; Glassview needs XInput 2.1",
                version.major_version, version.minor_version
            )
            .into());
        }
        xkb::use_extension(&conn, 1, 0)?.reply()?;
        xinput::xi_select_events(
            &conn,
            root,
            &[xinput::EventMask {
                deviceid: Device::ALL_MASTER.into(),
                mask: vec![
                    XIEventMask::RAW_KEY_PRESS
                        | XIEventMask::RAW_BUTTON_PRESS
                        | XIEventMask::RAW_MOTION,
                ],
            }],
        )?
        .check()?;
        let keymap = Keymap::load(&conn)?;
        Ok(Self { conn, root, keymap })
    }

    fn run(mut self, sink: &Sink) {
        loop {
            let event = match self.conn.wait_for_event() {
                Ok(event) => event,
                Err(error) => {
                    log::error!("Lost the X11 connection, so input capture stopped: {error}");
                    return;
                }
            };
            match event {
                Event::MappingNotify(notify) if notify.request == Mapping::KEYBOARD => {
                    match Keymap::load(&self.conn) {
                        Ok(keymap) => self.keymap = keymap,
                        Err(error) => log::warn!("Could not reload the keyboard map: {error}"),
                    }
                }
                _ if !sink.gate.enabled() => {}
                Event::XinputRawKeyPress(press) => self.key(sink, press.detail),
                Event::XinputRawButtonPress(press) => self.button(sink, press.detail),
                Event::XinputRawMotion(_) if sink.gate.halo() => {
                    if let Ok((at, _)) = self.pointer() {
                        sink.moved(at);
                    }
                }
                _ => {}
            }
        }
    }

    fn pointer(&self) -> Fallible<(Point, Mods)> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        let at = Point {
            x: f64::from(reply.root_x),
            y: f64::from(reply.root_y),
        };
        Ok((at, held_mods(reply.mask.into())))
    }

    fn key(&self, sink: &Sink, detail: u32) {
        let Ok(keycode) = Keycode::try_from(detail) else {
            return;
        };
        let (state, at) = match self.key_state() {
            Ok(state) => state,
            Err(error) => {
                log::debug!("Could not read the keyboard state: {error}");
                return;
            }
        };
        let group = usize::from(u8::from(state.group));
        let Some(base) = self.keymap.keysym(keycode, group, 0) else {
            return;
        };
        if is_modifier(base) {
            return;
        }
        let held: u16 = state.mods.into();
        let mods = held_mods(held);
        let mode = sink.gate.mode();
        if !keys::may_show(mods, mode) {
            return;
        }
        let Some(key) = named_key(base)
            .map(Key::Named)
            .or_else(|| to_char(base).map(Key::Char))
        else {
            return;
        };
        let text = keys::linux_typed_text(mods, || {
            self.keymap
                .keysym(keycode, group, level(base, held))
                .and_then(to_char)
        });
        if let Some(stroke) = keys::admit(KeyPress { mods, key, text }, mode) {
            sink.key(stroke, at);
        }
    }

    fn key_state(&self) -> Fallible<(GetStateReply, Point)> {
        let state = self.conn.xkb_get_state(xkb::ID::USE_CORE_KBD.into())?;
        let pointer = self.conn.query_pointer(self.root)?;
        let state = state.reply()?;
        let pointer = pointer.reply()?;
        let at = Point {
            x: f64::from(pointer.root_x),
            y: f64::from(pointer.root_y),
        };
        Ok((state, at))
    }

    fn button(&self, sink: &Sink, detail: u32) {
        let Ok((at, mods)) = self.pointer() else {
            return;
        };
        if let Some(button) = Button::from_x11(detail) {
            sink.press(button, mods, at);
        } else if let Some(delta) = ScrollDelta::from_x11_wheel(detail) {
            sink.scroll(at, delta);
        }
    }
}

/// The core keyboard mapping, which the server flattens from XKB: two columns per
/// group, then group 1's third and fourth levels from column 4.
struct Keymap {
    first: Keycode,
    per_keycode: usize,
    keysyms: Vec<Keysym>,
}
impl Keymap {
    fn load(conn: &RustConnection) -> Fallible<Self> {
        let (first, last) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let reply = conn
            .get_keyboard_mapping(first, last - first + 1)?
            .reply()?;
        Ok(Self {
            first,
            per_keycode: usize::from(reply.keysyms_per_keycode),
            keysyms: reply.keysyms,
        })
    }
    fn columns(&self, keycode: Keycode) -> &[Keysym] {
        let start = usize::from(keycode.wrapping_sub(self.first)) * self.per_keycode;
        self.keysyms
            .get(start..start + self.per_keycode)
            .unwrap_or(&[])
    }
    /// A missing shifted symbol reads as the unshifted one and a missing group or
    /// third level reads as group 1, the way the core protocol fills gaps.
    fn keysym(&self, keycode: Keycode, group: usize, level: usize) -> Option<Keysym> {
        let columns = self.columns(keycode);
        let column = |column: usize| columns.get(column).copied().filter(|k| *k != NO_SYMBOL);
        let shift = level & 1;
        let base = if level >= 2 && group == 0 {
            4
        } else {
            2 * group
        };
        column(base + shift)
            .or_else(|| column(base))
            .or_else(|| column(shift))
            .or_else(|| column(0))
    }
}

/// The level the modifiers select, as XKB's ALPHABETIC and KEYPAD key types do:
/// Caps Lock shifts letters, Num Lock shifts keypad keys, each cancelling against
/// Shift, and ISO_Level3_Shift (AltGr) picks the third pair.
fn level(base: Keysym, held: u16) -> usize {
    let bit = |mask: ModMask| held & u16::from(mask) != 0;
    let toggled = if is_keypad(base) {
        bit(ModMask::M2)
    } else {
        bit(ModMask::LOCK) && to_char(base).is_some_and(char::is_alphabetic)
    };
    usize::from(bit(ModMask::SHIFT) != toggled) + if bit(ModMask::M5) { 2 } else { 0 }
}

/// Mod1 is Alt and Mod4 is Super in every XKB rule set the desktops ship.
fn held_mods(held: u16) -> Mods {
    [
        (ModMask::CONTROL, Mods::CTRL),
        (ModMask::M1, Mods::ALT),
        (ModMask::SHIFT, Mods::SHIFT),
        (ModMask::M4, Mods::META),
    ]
    .into_iter()
    .filter(|(mask, _)| held & u16::from(*mask) != 0)
    .fold(Mods::NONE, |mods, (_, m)| mods | m)
}

fn to_char(keysym: Keysym) -> Option<char> {
    gtk::gdk::keys::Key::from(keysym).to_unicode()
}

fn is_keypad(keysym: Keysym) -> bool {
    (key::KP_Space..=key::KP_Equal).contains(&keysym)
}

fn is_modifier(keysym: Keysym) -> bool {
    matches!(
        keysym,
        key::Shift_L
            ..=key::Hyper_R
                | key::ISO_Level3_Shift
                | key::ISO_Level5_Shift
                | key::Mode_switch
                | key::Num_Lock
                | key::Scroll_Lock
    )
}

fn named_key(keysym: Keysym) -> Option<Named> {
    Some(match keysym {
        key::Return | key::KP_Enter => Named::Enter,
        key::Tab | key::KP_Tab => Named::Tab,
        key::space | key::KP_Space => Named::Space,
        key::BackSpace => Named::Backspace,
        key::Delete | key::KP_Delete => Named::Delete,
        key::Escape => Named::Escape,
        key::Insert | key::KP_Insert => Named::Insert,
        key::Left | key::KP_Left => Named::Left,
        key::Right | key::KP_Right => Named::Right,
        key::Up | key::KP_Up => Named::Up,
        key::Down | key::KP_Down => Named::Down,
        key::Home | key::KP_Home => Named::Home,
        key::End | key::KP_End => Named::End,
        key::Page_Up | key::KP_Page_Up => Named::PageUp,
        key::Page_Down | key::KP_Page_Down => Named::PageDown,
        key::F1..=key::F24 => Named::F((keysym - key::F1 + 1) as u8),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap() -> Keymap {
        Keymap {
            first: 8,
            per_keycode: 6,
            keysyms: [
                [
                    key::q,
                    key::Q,
                    key::q,
                    key::Q,
                    key::adiaeresis,
                    key::Adiaeresis,
                ],
                [
                    key::Return,
                    NO_SYMBOL,
                    key::Return,
                    NO_SYMBOL,
                    NO_SYMBOL,
                    NO_SYMBOL,
                ],
            ]
            .concat(),
        }
    }

    #[test]
    fn keysyms_come_from_the_group_and_level_with_core_fallbacks() {
        let map = keymap();
        assert_eq!(map.keysym(8, 0, 0), Some(key::q));
        assert_eq!(map.keysym(8, 0, 1), Some(key::Q));
        assert_eq!(map.keysym(8, 0, 2), Some(key::adiaeresis), "AltGr");
        assert_eq!(map.keysym(8, 0, 3), Some(key::Adiaeresis), "Shift+AltGr");
        assert_eq!(
            map.keysym(9, 0, 1),
            Some(key::Return),
            "Shift+Return is Return"
        );
        assert_eq!(map.keysym(9, 0, 2), Some(key::Return), "no third level");
        assert_eq!(
            map.keysym(9, 1, 0),
            Some(key::Return),
            "second group reads the first"
        );
        assert_eq!(
            map.keysym(9, 5, 0),
            Some(key::Return),
            "a group past the map too"
        );
        assert_eq!(map.keysym(7, 0, 0), None, "below the first keycode");
        assert_eq!(map.keysym(10, 0, 0), None, "past the last keycode");
    }

    #[test]
    fn locks_cancel_against_shift_for_the_keys_they_govern() {
        let (shift, lock, num_lock, level3) = (
            u16::from(ModMask::SHIFT),
            u16::from(ModMask::LOCK),
            u16::from(ModMask::M2),
            u16::from(ModMask::M5),
        );
        assert_eq!(level(key::q, 0), 0);
        assert_eq!(level(key::q, shift), 1);
        assert_eq!(level(key::q, lock), 1, "Caps Lock shifts a letter");
        assert_eq!(level(key::q, shift | lock), 0);
        assert_eq!(level(key::_1, lock), 0, "but not a digit");
        assert_eq!(level(key::_1, shift | lock), 1);
        assert_eq!(
            level(key::KP_End, num_lock),
            1,
            "Num Lock shifts KP_End to KP_1"
        );
        assert_eq!(level(key::KP_End, shift | num_lock), 0);
        assert_eq!(level(key::q, level3), 2);
        assert_eq!(level(key::q, level3 | shift), 3);
    }

    #[test]
    fn modifier_bits_map_to_mods_and_modifier_keys_are_skipped() {
        let all = u16::from(ModMask::SHIFT)
            | u16::from(ModMask::CONTROL)
            | u16::from(ModMask::M1)
            | u16::from(ModMask::M4);
        assert_eq!(
            held_mods(all),
            Mods::SHIFT | Mods::CTRL | Mods::ALT | Mods::META
        );
        assert_eq!(
            held_mods(u16::from(ModMask::M5)),
            Mods::NONE,
            "AltGr is not a command"
        );
        assert_eq!(held_mods(u16::from(ModMask::LOCK)), Mods::NONE);
        for keysym in [
            key::Shift_L,
            key::Control_L,
            key::Alt_L,
            key::Super_L,
            key::Caps_Lock,
            key::ISO_Level3_Shift,
            key::Num_Lock,
        ] {
            assert!(is_modifier(keysym), "{keysym:#x}");
        }
        assert!(!is_modifier(key::q));
        assert!(!is_modifier(key::Return));
    }

    #[test]
    fn keysyms_name_keys_and_characters() {
        assert_eq!(named_key(key::Return), Some(Named::Enter));
        assert_eq!(named_key(key::KP_Enter), Some(Named::Enter));
        assert_eq!(named_key(key::KP_End), Some(Named::End));
        assert_eq!(named_key(key::F1), Some(Named::F(1)));
        assert_eq!(named_key(key::F12), Some(Named::F(12)));
        assert_eq!(named_key(key::F24), Some(Named::F(24)));
        assert_eq!(named_key(key::q), None);
        assert_eq!(to_char(key::q), Some('q'));
        assert_eq!(to_char(key::adiaeresis), Some('ä'));
        assert_eq!(to_char(0x0100_0000 | u32::from('є')), Some('є'));
        assert_eq!(to_char(key::KP_1), Some('1'));
        assert_eq!(to_char(key::dead_acute), None);
    }
}
