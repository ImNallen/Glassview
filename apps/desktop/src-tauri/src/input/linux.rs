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
        xkb::{self, ConnectionExt as _, EventType, KeySymMap, KeyType, MapPart},
        xproto::{ConnectionExt as _, Keycode, Keysym, Mapping, ModMask, Window},
        Event,
    },
    rust_connection::RustConnection,
};
use xkeysym::key;

type Fallible<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

const NO_SYMBOL: Keysym = 0;

/// XInput2 raw events on the root window report every key and button without a
/// grab, but carry no position or modifier state. XKB state events arrive on the
/// same connection in order, so the modifiers are the ones held at each press.
pub(super) fn start(sink: Sink) -> Result<()> {
    let set = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    // XWayland sets DISPLAY too, but only sees input aimed at X11 windows.
    if set("WAYLAND_DISPLAY") || !set("DISPLAY") {
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct State {
    mods: u16,
    group: u8,
}

struct Watcher {
    conn: RustConnection,
    root: Window,
    keymap: Keymap,
    buttons: Vec<u8>,
    state: State,
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
        let keyboard = xkb::ID::USE_CORE_KBD.into();
        let events =
            EventType::STATE_NOTIFY | EventType::MAP_NOTIFY | EventType::NEW_KEYBOARD_NOTIFY;
        let parts = MapPart::KEY_TYPES | MapPart::KEY_SYMS;
        conn.xkb_select_events(
            keyboard,
            EventType::from(0u16),
            events,
            parts,
            parts,
            &xkb::SelectEventsAux::new(),
        )?
        .check()?;
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
        let state = conn.xkb_get_state(keyboard)?.reply()?;
        let state = State {
            mods: state.mods.into(),
            group: state.group.into(),
        };
        let keymap = Keymap::load(&conn)?;
        let buttons = conn.get_pointer_mapping()?.reply()?.map;
        Ok(Self {
            conn,
            root,
            keymap,
            buttons,
            state,
        })
    }

    fn run(mut self, sink: &Sink) {
        let mut moved = false;
        loop {
            // A fast mouse sends motion faster than the pointer can be queried, so
            // the halo follows the last one in each burst.
            let event = match self.conn.poll_for_event() {
                Ok(Some(event)) => Ok(event),
                Ok(None) => {
                    if std::mem::take(&mut moved) {
                        if let Ok(at) = self.pointer() {
                            sink.moved(at);
                        }
                    }
                    self.conn.wait_for_event()
                }
                Err(error) => Err(error),
            };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    log::error!("Lost the X11 connection, so input capture stopped: {error}");
                    return;
                }
            };
            match event {
                Event::XkbStateNotify(notify) => {
                    self.state = State {
                        mods: notify.mods.into(),
                        group: notify.group.into(),
                    }
                }
                Event::XkbMapNotify(_) | Event::XkbNewKeyboardNotify(_) => self.reload_keymap(),
                Event::MappingNotify(notify) if notify.request == Mapping::KEYBOARD => {
                    self.reload_keymap()
                }
                Event::MappingNotify(notify) if notify.request == Mapping::POINTER => {
                    match self.conn.get_pointer_mapping().map(|cookie| cookie.reply()) {
                        Ok(Ok(reply)) => self.buttons = reply.map,
                        _ => log::warn!("Could not reload the pointer button map"),
                    }
                }
                _ if !sink.gate.enabled() => {}
                Event::XinputRawKeyPress(press) => self.key(sink, press.detail),
                Event::XinputRawButtonPress(press) => self.button(sink, press.detail),
                Event::XinputRawMotion(_) => moved = sink.gate.halo(),
                _ => {}
            }
        }
    }

    fn reload_keymap(&mut self) {
        match Keymap::load(&self.conn) {
            Ok(keymap) => self.keymap = keymap,
            Err(error) => log::warn!("Could not reload the keyboard map: {error}"),
        }
    }

    fn pointer(&self) -> Fallible<Point> {
        let reply = self.conn.query_pointer(self.root)?.reply()?;
        Ok(Point {
            x: f64::from(reply.root_x),
            y: f64::from(reply.root_y),
        })
    }

    fn key(&self, sink: &Sink, detail: u32) {
        let Ok(keycode) = Keycode::try_from(detail) else {
            return;
        };
        let mods = held_mods(self.state.mods);
        let mode = sink.gate.mode();
        if !keys::may_show(mods, mode) {
            return;
        }
        let Some(key) = self.keymap.label(keycode, self.state, mods.has_command()) else {
            return;
        };
        let text = keys::linux_typed_text(mods, || {
            self.keymap.keysym(keycode, self.state).and_then(to_char)
        });
        let Ok(at) = self.pointer() else {
            return;
        };
        if let Some(stroke) = keys::admit(KeyPress { mods, key, text }, mode) {
            sink.key(stroke, at);
        }
    }

    /// Raw events report the physical button, before the pointer mapping that
    /// swaps buttons for left-handed use or reverses the wheel.
    fn button(&self, sink: &Sink, detail: u32) {
        let detail = usize::try_from(detail)
            .ok()
            .and_then(|physical| self.buttons.get(physical.checked_sub(1)?))
            .map_or(detail, |&logical| u32::from(logical));
        let Ok(at) = self.pointer() else {
            return;
        };
        if let Some(button) = Button::from_x11(detail) {
            sink.press(button, held_mods(self.state.mods), at);
        } else if let Some(delta) = ScrollDelta::from_x11_wheel(detail) {
            sink.scroll(at, delta);
        }
    }
}

/// The XKB map: the symbols on each key by group and level, and the key types
/// that say which modifiers select each level.
struct Keymap {
    first: Keycode,
    types: Vec<KeyType>,
    keys: Vec<KeySymMap>,
}
impl Keymap {
    fn load(conn: &RustConnection) -> Fallible<Self> {
        let reply = conn
            .xkb_get_map(
                xkb::ID::USE_CORE_KBD.into(),
                MapPart::KEY_TYPES | MapPart::KEY_SYMS,
                MapPart::from(0u16),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0u16.into(),
                0,
                0,
                0,
                0,
                0,
                0,
            )?
            .reply()?;
        Ok(Self {
            first: reply.first_key_sym,
            types: reply.map.types_rtrn.unwrap_or_default(),
            keys: reply.map.syms_rtrn.unwrap_or_default(),
        })
    }

    fn keysym(&self, keycode: Keycode, state: State) -> Option<Keysym> {
        let key = self
            .keys
            .get(usize::from(keycode.checked_sub(self.first)?))?;
        let group = key_group(key.group_info, state.group)?;
        let kind = self.types.get(usize::from(key.kt_index[group]))?;
        let mask = u16::from(kind.mods_mask);
        let level = kind
            .map
            .iter()
            .find(|entry| entry.active && state.mods & mask == u16::from(entry.mods_mask))
            .map_or(0, |entry| usize::from(entry.level));
        key.syms
            .get(group * usize::from(key.width) + level)
            .copied()
            .filter(|keysym| *keysym != NO_SYMBOL)
    }

    /// Names the key the way a chord or a key pill shows it: unshifted, but on the
    /// level Num Lock or AltGr picks. Shortcuts typed on a non-Latin layout read
    /// as the Latin letter apps match them against.
    fn label(&self, keycode: Keycode, state: State, command: bool) -> Option<Key> {
        let case_and_commands =
            ModMask::SHIFT | ModMask::LOCK | ModMask::CONTROL | ModMask::M1 | ModMask::M4;
        let shown = State {
            mods: state.mods & !u16::from(case_and_commands),
            ..state
        };
        let key = self.keysym(keycode, shown).and_then(name)?;
        if !command || !matches!(key, Key::Char(c) if !c.is_ascii()) {
            return Some(key);
        }
        let groups = self
            .keys
            .get(usize::from(keycode.checked_sub(self.first)?))?
            .group_info
            & 0x0f;
        (0..groups)
            .filter_map(|group| self.keysym(keycode, State { mods: 0, group }))
            .filter_map(name)
            .find(|latin| matches!(latin, Key::Char(c) if c.is_ascii()))
            .or(Some(key))
    }
}

/// A group the key lacks folds back into range as the key's group info says:
/// wrap by default, or clamp, or redirect to a fixed group.
fn key_group(info: u8, group: u8) -> Option<usize> {
    let count = info & 0x0f;
    let group = match info & 0xc0 {
        _ if count == 0 => return None,
        _ if group < count => group,
        0x40 => count - 1,
        0x80 => Some((info >> 4) & 0x03)
            .filter(|target| *target < count)
            .unwrap_or(0),
        _ => group % count,
    };
    Some(usize::from(group))
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

fn name(keysym: Keysym) -> Option<Key> {
    named_key(keysym)
        .map(Key::Named)
        .or_else(|| to_char(keysym).map(Key::Char))
}

fn to_char(keysym: Keysym) -> Option<char> {
    xkeysym::Keysym::from(keysym)
        .key_char()
        .filter(|c| !c.is_control())
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
    use xkb::KTMapEntry;

    fn mask(mods: &[ModMask]) -> u16 {
        mods.iter().fold(0, |all, m| all | u16::from(*m))
    }

    fn key_type(levels: &[(&[ModMask], u8)]) -> KeyType {
        let all: Vec<ModMask> = levels
            .iter()
            .flat_map(|(mods, _)| mods.iter().copied())
            .collect();
        KeyType {
            mods_mask: mask(&all).into(),
            mods_mods: mask(&all).into(),
            mods_vmods: 0u16.into(),
            num_levels: levels.iter().map(|(_, level)| level + 1).max().unwrap_or(1),
            has_preserve: false,
            map: levels
                .iter()
                .map(|(mods, level)| KTMapEntry {
                    active: true,
                    mods_mask: mask(mods).into(),
                    level: *level,
                    mods_mods: mask(mods).into(),
                    mods_vmods: 0u16.into(),
                })
                .collect(),
            preserve: Vec::new(),
        }
    }

    fn key(types: [u8; 4], group_info: u8, width: u8, syms: &[Keysym]) -> KeySymMap {
        KeySymMap {
            kt_index: types,
            group_info,
            width,
            syms: syms.to_vec(),
        }
    }

    const ONE_LEVEL: u8 = 0;
    const ALPHABETIC: u8 = 1;
    const FOUR_LEVEL_ALPHABETIC: u8 = 2;
    const KEYPAD: u8 = 3;

    /// Keycode 8 is Q on us,de; 9 is C on us,ru; 10 is keypad 1; 11 is Return.
    fn keymap() -> Keymap {
        use ModMask as M;
        Keymap {
            first: 8,
            types: vec![
                key_type(&[]),
                key_type(&[(&[M::SHIFT], 1), (&[M::LOCK], 1)]),
                key_type(&[
                    (&[M::SHIFT], 1),
                    (&[M::LOCK], 1),
                    (&[M::M5], 2),
                    (&[M::SHIFT, M::M5], 3),
                    (&[M::LOCK, M::M5], 3),
                ]),
                key_type(&[(&[M::SHIFT], 1), (&[M::M2], 1)]),
            ],
            keys: vec![
                key(
                    [ALPHABETIC, FOUR_LEVEL_ALPHABETIC, 0, 0],
                    2,
                    4,
                    &[
                        key::q,
                        key::Q,
                        NO_SYMBOL,
                        NO_SYMBOL,
                        key::q,
                        key::Q,
                        key::at,
                        key::Greek_OMEGA,
                    ],
                ),
                key(
                    [ALPHABETIC, ALPHABETIC, 0, 0],
                    2,
                    2,
                    &[key::c, key::C, key::Cyrillic_es, key::Cyrillic_ES],
                ),
                key([KEYPAD, 0, 0, 0], 1, 2, &[key::KP_End, key::KP_1]),
                key([ONE_LEVEL, 0, 0, 0], 1, 1, &[key::Return]),
            ],
        }
    }

    fn state(mods: &[ModMask], group: u8) -> State {
        State {
            mods: mask(mods),
            group,
        }
    }

    #[test]
    fn key_types_pick_the_level_within_the_active_group() {
        use ModMask as M;
        let map = keymap();
        assert_eq!(map.keysym(8, state(&[], 0)), Some(key::q));
        assert_eq!(map.keysym(8, state(&[M::SHIFT], 0)), Some(key::Q));
        assert_eq!(
            map.keysym(8, state(&[M::LOCK], 0)),
            Some(key::Q),
            "Caps Lock"
        );
        assert_eq!(map.keysym(8, state(&[M::SHIFT, M::LOCK], 0)), Some(key::q));
        assert_eq!(
            map.keysym(8, state(&[M::M5], 0)),
            Some(key::q),
            "us has no AltGr level"
        );
        assert_eq!(
            map.keysym(8, state(&[M::M5], 1)),
            Some(key::at),
            "AltGr on de"
        );
        assert_eq!(
            map.keysym(8, state(&[M::M5, M::SHIFT], 1)),
            Some(key::Greek_OMEGA)
        );
        assert_eq!(map.keysym(9, state(&[], 1)), Some(key::Cyrillic_es));
        assert_eq!(
            map.keysym(10, state(&[M::M2], 0)),
            Some(key::KP_1),
            "Num Lock"
        );
        assert_eq!(
            map.keysym(10, state(&[M::M2, M::SHIFT], 0)),
            Some(key::KP_End)
        );
        assert_eq!(map.keysym(11, state(&[M::SHIFT], 0)), Some(key::Return));
        assert_eq!(
            map.keysym(7, state(&[], 0)),
            None,
            "below the first keycode"
        );
        assert_eq!(map.keysym(12, state(&[], 0)), None, "past the last keycode");
    }

    #[test]
    fn groups_a_key_lacks_fold_back_into_range() {
        assert_eq!(key_group(1, 1), Some(0), "wrap");
        assert_eq!(key_group(2, 3), Some(1), "wrap");
        assert_eq!(key_group(0x40 | 2, 3), Some(1), "clamp");
        assert_eq!(key_group(0x80 | 0x10 | 3, 3), Some(1), "redirect");
        assert_eq!(
            key_group(0x80 | 0x30 | 2, 3),
            Some(0),
            "redirect out of range"
        );
        assert_eq!(key_group(0, 0), None, "no groups");
        assert_eq!(keymap().keysym(11, state(&[], 3)), Some(key::Return));
    }

    #[test]
    fn labels_drop_case_and_commands_but_keep_num_lock_and_altgr() {
        use ModMask as M;
        let map = keymap();
        let label =
            |keycode, mods: &[M], group, command| map.label(keycode, state(mods, group), command);
        assert_eq!(
            label(8, &[M::SHIFT, M::CONTROL], 0, true),
            Some(Key::Char('q'))
        );
        assert_eq!(label(8, &[M::LOCK], 0, false), Some(Key::Char('q')));
        assert_eq!(
            label(8, &[M::M5, M::CONTROL], 1, true),
            Some(Key::Char('@'))
        );
        assert_eq!(
            label(10, &[M::M2, M::CONTROL], 0, true),
            Some(Key::Char('1'))
        );
        assert_eq!(
            label(10, &[M::CONTROL], 0, true),
            Some(Key::Named(Named::End))
        );
        assert_eq!(label(11, &[], 0, false), Some(Key::Named(Named::Enter)));
        assert_eq!(
            label(9, &[M::CONTROL], 1, true),
            Some(Key::Char('c')),
            "a shortcut on ru reads as Latin"
        );
        assert_eq!(
            label(9, &[], 1, false),
            Some(Key::Char('с')),
            "typing on ru does not"
        );
    }

    #[test]
    fn modifier_bits_map_to_mods() {
        assert_eq!(
            held_mods(mask(&[
                ModMask::SHIFT,
                ModMask::CONTROL,
                ModMask::M1,
                ModMask::M4
            ])),
            Mods::SHIFT | Mods::CTRL | Mods::ALT | Mods::META
        );
        assert_eq!(
            held_mods(mask(&[ModMask::M5])),
            Mods::NONE,
            "AltGr is not a command"
        );
        assert_eq!(held_mods(mask(&[ModMask::LOCK])), Mods::NONE);
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
        assert_eq!(to_char(key::Clear), None, "control characters");
        assert_eq!(name(key::Shift_L), None, "modifiers alone are not keys");
        assert_eq!(name(key::ISO_Level3_Shift), None);
    }
}
