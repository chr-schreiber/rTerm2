use alacritty_terminal::term::TermMode;
use winit::event::KeyEvent;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

use crate::keymap::Mods;

/// Modifikatoren für die Kürzel-Suche. `altgr`: AltGr gedrückt (winit meldet es nicht in
/// `ModifiersState`; bei „Deutsch (Macintosh)“ sind beide Alt-Tasten AltGr).
pub fn mods(state: ModifiersState, altgr: bool) -> Mods {
    Mods {
        ctrl: state.control_key(),
        shift: state.shift_key(),
        alt: state.alt_key(),
        altgr,
        sup: state.super_key(),
    }
}

/// Name der Taste ohne Modifikator-Wirkung, passend zu `keymap::parse_spec`:
/// Zeichen klein (`d`, `+`, `1`), sonst `enter`, `left`, `pageup`, `f5` …
/// Ohne Modifikatoren, damit z. B. Ctrl+Shift+D unabhängig vom Layout als „d“ ankommt.
pub fn key_name(event: &KeyEvent) -> Option<String> {
    let base = event.key_without_modifiers();
    let name = match &base {
        Key::Character(s) => return Some(s.to_lowercase()),
        Key::Named(n) => match n {
            NamedKey::Enter => "enter",
            NamedKey::Tab => "tab",
            NamedKey::Space => "space",
            NamedKey::Escape => "escape",
            NamedKey::Backspace => "backspace",
            NamedKey::Delete => "delete",
            NamedKey::Insert => "insert",
            NamedKey::Home => "home",
            NamedKey::End => "end",
            NamedKey::PageUp => "pageup",
            NamedKey::PageDown => "pagedown",
            NamedKey::ArrowLeft => "left",
            NamedKey::ArrowRight => "right",
            NamedKey::ArrowUp => "up",
            NamedKey::ArrowDown => "down",
            NamedKey::F1 => "f1",
            NamedKey::F2 => "f2",
            NamedKey::F3 => "f3",
            NamedKey::F4 => "f4",
            NamedKey::F5 => "f5",
            NamedKey::F6 => "f6",
            NamedKey::F7 => "f7",
            NamedKey::F8 => "f8",
            NamedKey::F9 => "f9",
            NamedKey::F10 => "f10",
            NamedKey::F11 => "f11",
            NamedKey::F12 => "f12",
            _ => return None,
        },
        _ => return None,
    };
    Some(name.to_string())
}

/// Übersetzt einen Tastendruck in die Bytes, die an das PTY gehen.
/// Gibt `None` zurück, wenn die Taste nichts sendet.
pub fn key_to_bytes(event: &KeyEvent, mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    let ctrl = mods.control_key();
    let alt = mods.alt_key();
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    let mut out: Vec<u8> = match &event.logical_key {
        Key::Named(named) => named_key(*named, ctrl, mods.shift_key(), app_cursor)?,
        Key::Character(s) if ctrl => {
            let c = s.chars().next()?;
            match c.to_ascii_lowercase() {
                c @ 'a'..='z' => vec![c as u8 - b'a' + 1],
                '@' | '2' => vec![0],
                '[' | '3' => vec![0x1b],
                '\\' | '4' => vec![0x1c],
                ']' | '5' => vec![0x1d],
                '^' | '6' => vec![0x1e],
                '_' | '-' | '7' => vec![0x1f],
                '?' | '8' => vec![0x7f],
                _ => event.text.as_ref()?.as_bytes().to_vec(),
            }
        }
        _ => event.text.as_ref()?.as_bytes().to_vec(),
    };

    if out.is_empty() {
        return None;
    }
    // Alt als Meta: ESC voranstellen (wie iTerm2 mit "Option = Esc+").
    if alt
        && !matches!(
            event.logical_key,
            Key::Named(
                NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
            )
        )
    {
        out.insert(0, 0x1b);
    }
    Some(out)
}

fn named_key(key: NamedKey, ctrl: bool, shift: bool, app_cursor: bool) -> Option<Vec<u8>> {
    let csi_mod = modifier_param(ctrl, shift);
    let arrow = |c: u8| -> Vec<u8> {
        match csi_mod {
            Some(m) => format!("\x1b[1;{m}{}", c as char).into_bytes(),
            None if app_cursor => vec![0x1b, b'O', c],
            None => vec![0x1b, b'[', c],
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        match csi_mod {
            Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
            None => format!("\x1b[{n}~").into_bytes(),
        }
    };

    Some(match key {
        NamedKey::Enter => b"\r".to_vec(),
        NamedKey::Backspace => {
            if ctrl {
                vec![0x08]
            } else {
                vec![0x7f]
            }
        }
        NamedKey::Tab => {
            if shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        NamedKey::Escape => vec![0x1b],
        NamedKey::Space => {
            if ctrl {
                vec![0]
            } else {
                b" ".to_vec()
            }
        }
        NamedKey::ArrowUp => arrow(b'A'),
        NamedKey::ArrowDown => arrow(b'B'),
        NamedKey::ArrowRight => arrow(b'C'),
        NamedKey::ArrowLeft => arrow(b'D'),
        NamedKey::Home => arrow(b'H'),
        NamedKey::End => arrow(b'F'),
        NamedKey::Insert => tilde(2),
        NamedKey::Delete => tilde(3),
        NamedKey::PageUp => tilde(5),
        NamedKey::PageDown => tilde(6),
        NamedKey::F1 => b"\x1bOP".to_vec(),
        NamedKey::F2 => b"\x1bOQ".to_vec(),
        NamedKey::F3 => b"\x1bOR".to_vec(),
        NamedKey::F4 => b"\x1bOS".to_vec(),
        NamedKey::F5 => tilde(15),
        NamedKey::F6 => tilde(17),
        NamedKey::F7 => tilde(18),
        NamedKey::F8 => tilde(19),
        NamedKey::F9 => tilde(20),
        NamedKey::F10 => tilde(21),
        NamedKey::F11 => tilde(23),
        NamedKey::F12 => tilde(24),
        _ => return None,
    })
}

/// xterm-Modifikatorparameter (2 = Shift, 5 = Ctrl, 6 = Ctrl+Shift).
fn modifier_param(ctrl: bool, shift: bool) -> Option<u8> {
    match (ctrl, shift) {
        (false, false) => None,
        (false, true) => Some(2),
        (true, false) => Some(5),
        (true, true) => Some(6),
    }
}
