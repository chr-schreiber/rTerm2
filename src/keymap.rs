//! Tastenbelegung: Aktionen, Standard-Kürzel und `[keybindings]` aus der Config.
//! Ohne winit-Abhängigkeit, damit unit-testbar; `input.rs` übersetzt winit-Ereignisse in `(Taste, Mods)`.

use std::collections::HashMap;

use crate::layout::{Direction, SplitDir};

/// Aktionen der Anwendung, die per Tastenkürzel ausgelöst werden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    NewWindow,
    NewTab,
    NextTab,
    PrevTab,
    SelectTab(usize),
    Split(SplitDir),
    ClosePane,
    ToggleZoom,
    Focus(Direction),
    ResizeSplit(Direction),
    ScrollPageUp,
    ScrollPageDown,
    Copy,
    Paste,
    FontBigger,
    FontSmaller,
    FontReset,
    ThemePicker,
    Search,
    CopyMode,
}

const DIRS: [(&str, Direction); 4] = [
    ("left", Direction::Left),
    ("right", Direction::Right),
    ("up", Direction::Up),
    ("down", Direction::Down),
];

impl Action {
    /// Name in der Config, z. B. `split_right`, `focus_left`, `select_tab_3`.
    pub fn from_name(name: &str) -> Option<Action> {
        let simple = match name {
            "new_window" => Action::NewWindow,
            "new_tab" => Action::NewTab,
            "next_tab" => Action::NextTab,
            "prev_tab" => Action::PrevTab,
            "split_right" => Action::Split(SplitDir::Right),
            "split_down" => Action::Split(SplitDir::Down),
            "close_pane" => Action::ClosePane,
            "toggle_zoom" => Action::ToggleZoom,
            "scroll_page_up" => Action::ScrollPageUp,
            "scroll_page_down" => Action::ScrollPageDown,
            "copy" => Action::Copy,
            "paste" => Action::Paste,
            "font_bigger" => Action::FontBigger,
            "font_smaller" => Action::FontSmaller,
            "font_reset" => Action::FontReset,
            "theme_picker" => Action::ThemePicker,
            "search" => Action::Search,
            "copy_mode" => Action::CopyMode,
            _ => {
                if let Some(n) = name.strip_prefix("select_tab_") {
                    return n
                        .parse::<usize>()
                        .ok()
                        .filter(|n| (1..=9).contains(n))
                        .map(|n| Action::SelectTab(n - 1));
                }
                if let Some(d) = name.strip_prefix("focus_") {
                    return DIRS
                        .iter()
                        .find(|(s, _)| *s == d)
                        .map(|(_, d)| Action::Focus(*d));
                }
                if let Some(d) = name.strip_prefix("resize_") {
                    return DIRS
                        .iter()
                        .find(|(s, _)| *s == d)
                        .map(|(_, d)| Action::ResizeSplit(*d));
                }
                return None;
            }
        };
        Some(simple)
    }
}

/// Gedrückte Modifikatoren. `altgr` getrennt, weil winit AltGr nicht als Alt meldet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub altgr: bool,
    pub sup: bool,
}

impl Mods {
    /// Passt ein gedrückter Zustand zu diesem Kürzel? Ctrl, Shift, Alt und Super müssen genau
    /// stimmen. AltGr zählt nur, wenn das Kürzel es verlangt: Der AltGr-Zustand wird von rterm2
    /// selbst verfolgt und kann nach Fokuswechseln oder bei Mac-Layouts hängen bleiben – ein
    /// fälschlich „gedrücktes“ AltGr darf Ctrl+Shift+D & Co. nicht blockieren.
    pub fn matches(&self, pressed: Mods) -> bool {
        self.ctrl == pressed.ctrl
            && self.shift == pressed.shift
            && self.alt == pressed.alt
            && self.sup == pressed.sup
            && (!self.altgr || pressed.altgr)
    }
}

/// Ein Kürzel: Modifikatoren + Taste (Kleinbuchstabe/Zeichen oder Name wie `enter`, `left`, `f5`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeySpec {
    pub mods: Mods,
    pub key: String,
}

/// Liest z. B. `"ctrl+shift+d"`, `"alt+left"`, `"ctrl+plus"`. Groß/Klein egal.
pub fn parse_spec(spec: &str) -> Result<KeySpec, String> {
    let lower = spec.trim().to_lowercase();
    let parts: Vec<&str> = lower.split('+').map(str::trim).collect();
    let (key, mod_parts) = parts
        .split_last()
        .ok_or_else(|| format!("leeres Kürzel „{spec}“"))?;
    if key.is_empty() {
        return Err(format!(
            "„{spec}“: Taste fehlt (für + bitte „plus“ schreiben)"
        ));
    }
    let mut mods = Mods::default();
    for m in mod_parts {
        match *m {
            "ctrl" | "control" | "strg" => mods.ctrl = true,
            "shift" | "umschalt" => mods.shift = true,
            "alt" | "option" => mods.alt = true,
            "altgr" => mods.altgr = true,
            "super" | "meta" | "cmd" | "win" | "logo" => mods.sup = true,
            other => return Err(format!("„{spec}“: unbekannter Modifikator „{other}“")),
        }
    }
    let key = match *key {
        "plus" => "+",
        "minus" => "-",
        "equal" | "equals" => "=",
        "esc" => "escape",
        "return" => "enter",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        "arrowleft" => "left",
        "arrowright" => "right",
        "arrowup" => "up",
        "arrowdown" => "down",
        "del" => "delete",
        "ins" => "insert",
        k => k,
    };
    const NAMED: [&str; 13] = [
        "enter",
        "tab",
        "space",
        "escape",
        "backspace",
        "delete",
        "insert",
        "home",
        "end",
        "pageup",
        "pagedown",
        "left",
        "right",
    ];
    let is_fkey = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n));
    let valid =
        key.chars().count() == 1 || NAMED.contains(&key) || key == "up" || key == "down" || is_fkey;
    if !valid {
        return Err(format!("„{spec}“: unbekannte Taste „{key}“"));
    }
    Ok(KeySpec {
        mods,
        key: key.to_string(),
    })
}

/// Standardbelegung angelehnt an iTerm2 (⌘ → Ctrl+Shift, ⌥⌘ → Alt).
pub fn default_bindings() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![
        ("new_window", "ctrl+shift+n"),
        ("new_tab", "ctrl+shift+t"),
        ("next_tab", "ctrl+pagedown"),
        ("next_tab", "ctrl+tab"),
        ("prev_tab", "ctrl+pageup"),
        ("prev_tab", "ctrl+shift+tab"),
        ("split_right", "ctrl+shift+d"),
        ("split_down", "ctrl+shift+e"),
        ("close_pane", "ctrl+shift+w"),
        ("toggle_zoom", "ctrl+shift+enter"),
        ("focus_left", "ctrl+shift+h"),
        ("focus_down", "ctrl+shift+j"),
        ("focus_up", "ctrl+shift+k"),
        ("focus_right", "ctrl+shift+l"),
        ("scroll_page_up", "shift+pageup"),
        ("scroll_page_down", "shift+pagedown"),
        ("copy", "ctrl+shift+c"),
        ("paste", "ctrl+shift+v"),
        ("font_bigger", "ctrl+plus"),
        ("font_bigger", "ctrl+equal"),
        ("font_bigger", "ctrl+shift+plus"),
        ("font_bigger", "ctrl+shift+equal"),
        ("font_smaller", "ctrl+minus"),
        ("font_reset", "ctrl+0"),
        ("theme_picker", "ctrl+shift+p"),
        ("search", "ctrl+shift+f"),
        ("copy_mode", "ctrl+shift+space"),
    ];
    const TABS: [(&str, &str); 9] = [
        ("select_tab_1", "alt+1"),
        ("select_tab_2", "alt+2"),
        ("select_tab_3", "alt+3"),
        ("select_tab_4", "alt+4"),
        ("select_tab_5", "alt+5"),
        ("select_tab_6", "alt+6"),
        ("select_tab_7", "alt+7"),
        ("select_tab_8", "alt+8"),
        ("select_tab_9", "alt+9"),
    ];
    v.extend(TABS);
    for (dir, key) in [
        ("left", "left"),
        ("right", "right"),
        ("up", "up"),
        ("down", "down"),
    ] {
        let focus: &'static str = match dir {
            "left" => "focus_left",
            "right" => "focus_right",
            "up" => "focus_up",
            _ => "focus_down",
        };
        let resize: &'static str = match dir {
            "left" => "resize_left",
            "right" => "resize_right",
            "up" => "resize_up",
            _ => "resize_down",
        };
        let specs: [&'static str; 5] = match key {
            "left" => [
                "alt+left",
                "altgr+left",
                "ctrl+alt+left",
                "ctrl+altgr+left",
                "ctrl+shift+left",
            ],
            "right" => [
                "alt+right",
                "altgr+right",
                "ctrl+alt+right",
                "ctrl+altgr+right",
                "ctrl+shift+right",
            ],
            "up" => [
                "alt+up",
                "altgr+up",
                "ctrl+alt+up",
                "ctrl+altgr+up",
                "ctrl+shift+up",
            ],
            _ => [
                "alt+down",
                "altgr+down",
                "ctrl+alt+down",
                "ctrl+altgr+down",
                "ctrl+shift+down",
            ],
        };
        for s in &specs[..4] {
            v.push((focus, s));
        }
        v.push((resize, specs[4]));
    }
    v
}

pub struct Keymap {
    bindings: Vec<(KeySpec, Action)>,
}

impl Keymap {
    /// Standardbelegung + Änderungen aus der Config. Ein Eintrag ersetzt alle Standard-Kürzel
    /// seiner Aktion; `"none"` oder `[]` schaltet die Aktion ab. Gibt Warnungen zurück.
    pub fn new(overrides: &HashMap<String, Vec<String>>) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut bindings: Vec<(KeySpec, Action)> = default_bindings()
            .into_iter()
            .map(|(a, s)| {
                (
                    parse_spec(s).expect("Standard-Kürzel gültig"),
                    Action::from_name(a).expect("Standard-Aktion gültig"),
                )
            })
            .collect();

        let mut names: Vec<&String> = overrides.keys().collect();
        names.sort();
        for name in names {
            let Some(action) = Action::from_name(name) else {
                warnings.push(format!("[keybindings]: unbekannte Aktion „{name}“"));
                continue;
            };
            bindings.retain(|(_, a)| *a != action);
            for spec in &overrides[name] {
                if spec.trim().is_empty() || spec.trim().eq_ignore_ascii_case("none") {
                    continue;
                }
                match parse_spec(spec) {
                    Ok(ks) => {
                        // Gleiches Kürzel an anderer Aktion: die Config gewinnt.
                        bindings.retain(|(k, _)| *k != ks);
                        bindings.push((ks, action));
                    }
                    Err(e) => warnings.push(format!("[keybindings] {name}: {e}")),
                }
            }
        }
        (Keymap { bindings }, warnings)
    }

    pub fn lookup(&self, key: &str, mods: Mods) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(k, _)| k.mods.matches(mods) && k.key == key)
            .map(|(_, a)| *a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ctrl: bool, shift: bool, alt: bool) -> Mods {
        Mods {
            ctrl,
            shift,
            alt,
            ..Default::default()
        }
    }

    #[test]
    fn parsing() {
        assert_eq!(
            parse_spec("Ctrl+Shift+D").unwrap(),
            KeySpec {
                mods: m(true, true, false),
                key: "d".into()
            }
        );
        assert_eq!(parse_spec("ctrl + plus").unwrap().key, "+");
        assert_eq!(
            parse_spec("altgr+left").unwrap().mods,
            Mods {
                altgr: true,
                ..Default::default()
            }
        );
        assert_eq!(parse_spec("super+f12").unwrap().key, "f12");
        assert!(parse_spec("ctrl+").is_err());
        assert!(parse_spec("hyper+x").is_err());
        assert!(parse_spec("ctrl+banana").is_err());
        assert!(parse_spec("f13").is_err());
    }

    #[test]
    fn action_names() {
        assert_eq!(
            Action::from_name("select_tab_3"),
            Some(Action::SelectTab(2))
        );
        assert_eq!(Action::from_name("select_tab_0"), None);
        assert_eq!(
            Action::from_name("focus_up"),
            Some(Action::Focus(Direction::Up))
        );
        assert_eq!(
            Action::from_name("resize_left"),
            Some(Action::ResizeSplit(Direction::Left))
        );
        assert_eq!(
            Action::from_name("split_down"),
            Some(Action::Split(SplitDir::Down))
        );
        assert_eq!(Action::from_name("quatsch"), None);
    }

    #[test]
    fn defaults_complete_and_unique() {
        let (km, warnings) = Keymap::new(&HashMap::new());
        assert!(warnings.is_empty());
        for (i, (a, _)) in km.bindings.iter().enumerate() {
            assert!(
                !km.bindings[i + 1..].iter().any(|(b, _)| b == a),
                "doppelt: {a:?}"
            );
        }
        assert_eq!(
            km.lookup("d", m(true, true, false)),
            Some(Action::Split(SplitDir::Right))
        );
        assert_eq!(
            km.lookup(
                "left",
                Mods {
                    altgr: true,
                    ..Default::default()
                }
            ),
            Some(Action::Focus(Direction::Left))
        );
        assert_eq!(
            km.lookup("left", m(true, true, false)),
            Some(Action::ResizeSplit(Direction::Left))
        );
        assert_eq!(
            km.lookup("+", m(true, false, false)),
            Some(Action::FontBigger)
        );
        assert_eq!(km.lookup("d", m(true, false, false)), None);
    }

    #[test]
    fn stuck_altgr_does_not_block_shortcuts() {
        let (km, _) = Keymap::new(&HashMap::new());
        let stuck = Mods {
            ctrl: true,
            shift: true,
            altgr: true,
            ..Default::default()
        };
        assert_eq!(km.lookup("d", stuck), Some(Action::Split(SplitDir::Right)));
        // altgr+left braucht AltGr wirklich
        assert_eq!(km.lookup("left", Mods::default()), None);
        assert_eq!(
            km.lookup(
                "left",
                Mods {
                    altgr: true,
                    ..Default::default()
                }
            ),
            Some(Action::Focus(Direction::Left))
        );
        // ctrl+altgr+left vs. ctrl+alt+left
        assert_eq!(
            km.lookup(
                "left",
                Mods {
                    ctrl: true,
                    altgr: true,
                    ..Default::default()
                }
            ),
            Some(Action::Focus(Direction::Left))
        );
    }

    #[test]
    fn overrides() {
        let mut o = HashMap::new();
        o.insert("split_right".to_string(), vec!["super+d".to_string()]);
        o.insert("close_pane".to_string(), vec!["none".to_string()]);
        // Ctrl+Shift+T neu belegen: new_tab verliert es
        o.insert(
            "new_window".to_string(),
            vec!["ctrl+shift+t".to_string(), "ctrl+shift+quatsch".to_string()],
        );
        o.insert("tanzen".to_string(), vec!["ctrl+x".to_string()]);
        let (km, warnings) = Keymap::new(&o);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let sup = Mods {
            sup: true,
            ..Default::default()
        };
        assert_eq!(km.lookup("d", sup), Some(Action::Split(SplitDir::Right)));
        assert_eq!(km.lookup("d", m(true, true, false)), None);
        assert_eq!(km.lookup("w", m(true, true, false)), None);
        assert_eq!(
            km.lookup("t", m(true, true, false)),
            Some(Action::NewWindow)
        );
        assert_eq!(km.lookup("n", m(true, true, false)), None);
    }
}
