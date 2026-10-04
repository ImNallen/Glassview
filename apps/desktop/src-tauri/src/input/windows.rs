use super::Sink;
use crate::{
    keys::{self, Key, KeyPress, Mods, Named},
    layout::Point,
    pipeline::Button,
    Result,
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::{null, null_mut},
    sync::{mpsc, OnceLock},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetCursorPos, GetForegroundWindow, GetMessageW,
            GetWindowThreadProcessId, SetWindowsHookExW, HC_ACTION, KBDLLHOOKSTRUCT,
            LLKHF_EXTENDED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
            WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
        },
    },
};

static SINK: OnceLock<Sink> = OnceLock::new();

/// Low-level hooks need no admin rights, but they run on the installing thread's
/// message loop and Windows silently removes one that answers slowly, so they get
/// a thread of their own and their callbacks only read state and send.
pub(super) fn start(sink: Sink) -> Result<()> {
    if SINK.set(sink).is_err() {
        return Err("Input capture is already running".into());
    }
    let (ready, installed) = mpsc::channel();
    std::thread::Builder::new()
        .name("glassview-input".into())
        .spawn(move || unsafe {
            let module = GetModuleHandleW(null());
            let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
            let _ = ready.send(if keyboard.is_null() || mouse.is_null() {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                Ok(())
            });
            let mut msg = std::mem::zeroed::<MSG>();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        })?;
    installed
        .recv()?
        .map_err(|error| format!("Could not watch the keyboard and mouse: {error}"))?;
    log::info!("Installed the keyboard and mouse hooks");
    Ok(())
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let _ = catch_unwind(AssertUnwindSafe(|| on_key(event)));
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let _ = catch_unwind(AssertUnwindSafe(|| on_mouse(wparam as u32, event)));
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn on_mouse(message: u32, event: &MSLLHOOKSTRUCT) {
    let Some(sink) = SINK.get().filter(|sink| sink.gate.enabled()) else {
        return;
    };
    // Physical virtual-screen pixels, since Tauri makes the process per-monitor DPI aware.
    let at = Point {
        x: f64::from(event.pt.x),
        y: f64::from(event.pt.y),
    };
    match message {
        WM_LBUTTONDOWN => sink.press(Button::Left, held_mods(), at),
        WM_RBUTTONDOWN => sink.press(Button::Right, held_mods(), at),
        WM_MBUTTONDOWN => sink.press(Button::Middle, held_mods(), at),
        WM_LBUTTONUP => sink.release(Button::Left, at),
        WM_RBUTTONUP => sink.release(Button::Right, at),
        WM_MBUTTONUP => sink.release(Button::Middle, at),
        WM_MOUSEMOVE => sink.moved(at, sink.holding() && any_button_held()),
        _ => {}
    }
}

/// In a low-level hook the async state is the state before this event, which is
/// exactly what is held. It is global, so a release inside an elevated window
/// the hook cannot see still clears it.
fn held(key: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(key)) }) < 0
}

fn held_mods() -> Mods {
    [
        (VK_CONTROL, Mods::CTRL),
        (VK_MENU, Mods::ALT),
        (VK_SHIFT, Mods::SHIFT),
        (VK_LWIN, Mods::META),
        (VK_RWIN, Mods::META),
    ]
    .into_iter()
    .filter(|(key, _)| held(*key))
    .fold(Mods::NONE, |mods, (_, m)| mods | m)
}

fn any_button_held() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2]
        .into_iter()
        .any(held)
}

fn on_key(event: &KBDLLHOOKSTRUCT) {
    let Some(sink) = SINK.get().filter(|sink| sink.gate.enabled()) else {
        return;
    };
    let vk = event.vkCode as VIRTUAL_KEY;
    if is_modifier(vk) {
        return;
    }
    let mods = held_mods();
    let mode = sink.gate.mode();
    if !keys::may_show(mods, mode) {
        return;
    }
    let extended = event.flags & LLKHF_EXTENDED != 0;
    // The layout of the app being typed into, which can differ per thread.
    let layout =
        unsafe { GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut())) };
    let Some(key) = named_key(vk)
        .map(Key::Named)
        .or_else(|| base_char(vk, layout).map(Key::Char))
    else {
        return;
    };
    let text = keys::windows_typed_text(mods, numpad_digit(vk, extended), || {
        to_unicode(vk, event.scanCode, mods, layout)
    });
    if let Some(stroke) = keys::admit(KeyPress { mods, key, text }, mode) {
        if let Some(cursor) = cursor() {
            sink.key(stroke, cursor);
        }
    }
}

pub(super) fn cursor() -> Option<Point> {
    let mut at = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut at) } != 0).then(|| Point {
        x: f64::from(at.x),
        y: f64::from(at.y),
    })
}

fn is_modifier(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk,
        VK_SHIFT
            | VK_LSHIFT
            | VK_RSHIFT
            | VK_CONTROL
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_MENU
            | VK_LMENU
            | VK_RMENU
            | VK_LWIN
            | VK_RWIN
            | VK_CAPITAL
            | VK_NUMLOCK
            | VK_SCROLL
    )
}

fn named_key(vk: VIRTUAL_KEY) -> Option<Named> {
    Some(match vk {
        VK_RETURN => Named::Enter,
        VK_TAB => Named::Tab,
        VK_SPACE => Named::Space,
        VK_BACK => Named::Backspace,
        VK_DELETE => Named::Delete,
        VK_ESCAPE => Named::Escape,
        VK_INSERT => Named::Insert,
        VK_LEFT => Named::Left,
        VK_RIGHT => Named::Right,
        VK_UP => Named::Up,
        VK_DOWN => Named::Down,
        VK_HOME => Named::Home,
        VK_END => Named::End,
        VK_PRIOR => Named::PageUp,
        VK_NEXT => Named::PageDown,
        VK_F1..=VK_F24 => Named::F((vk - VK_F1 + 1) as u8),
        _ => return None,
    })
}

/// The digit a numpad key types in an Alt code. With NumLock off the keypad reports
/// navigation keys, told apart from the dedicated ones by the extended flag.
fn numpad_digit(vk: VIRTUAL_KEY, extended: bool) -> Option<char> {
    let digit = match vk {
        VK_NUMPAD0..=VK_NUMPAD9 => vk - VK_NUMPAD0,
        _ if extended => return None,
        VK_INSERT => 0,
        VK_END => 1,
        VK_DOWN => 2,
        VK_NEXT => 3,
        VK_LEFT => 4,
        VK_CLEAR => 5,
        VK_RIGHT => 6,
        VK_HOME => 7,
        VK_UP => 8,
        VK_PRIOR => 9,
        _ => return None,
    };
    char::from_digit(u32::from(digit), 10)
}

fn base_char(vk: VIRTUAL_KEY, layout: HKL) -> Option<char> {
    let mapped = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_CHAR, layout) };
    // The high bit flags a dead key; the character is in the low bits.
    char::from_u32(mapped & 0x7FFF).filter(|c| *c != '\0' && !c.is_control())
}

/// Flag 0x4 leaves the kernel's keyboard state alone, so a dead key the user
/// pressed still combines with their next key.
fn to_unicode(vk: VIRTUAL_KEY, scan: u32, mods: Mods, layout: HKL) -> Option<char> {
    const DOWN: u8 = 0x80;
    let mut state = [0u8; 256];
    for (held, keys) in [
        (Mods::SHIFT, [VK_SHIFT, VK_LSHIFT]),
        (Mods::CTRL, [VK_CONTROL, VK_LCONTROL]),
        (Mods::ALT, [VK_MENU, VK_LMENU]),
    ] {
        if mods.has(held) {
            for key in keys {
                state[usize::from(key)] = DOWN;
            }
        }
    }
    state[usize::from(VK_CAPITAL)] = (unsafe { GetKeyState(i32::from(VK_CAPITAL)) } & 1) as u8;
    let mut buffer = [0u16; 8];
    let written = unsafe {
        ToUnicodeEx(
            u32::from(vk),
            scan,
            state.as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as i32,
            0x4,
            layout,
        )
    };
    let length = usize::try_from(written).ok()?;
    let mut chars = char::decode_utf16(buffer[..length].iter().copied());
    match (chars.next(), chars.next()) {
        (Some(Ok(c)), None) => Some(c),
        _ => None,
    }
}
