use std::path::{Path, PathBuf};

use alacritty_terminal::vte::ansi::{Color, NamedColor};
use anyhow::{anyhow, Context};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn hex(v: u32) -> Self {
        Self {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    /// Format für softbuffer: 0x00RRGGBB
    pub fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    pub fn from_u32(v: u32) -> Self {
        Self::hex(v)
    }

    /// Mischt `fg` mit Deckkraft `alpha` (0..=255) über `self`.
    pub fn blend(self, fg: Rgb, alpha: u8) -> Rgb {
        let a = alpha as u16;
        let mix = |b: u8, f: u8| ((b as u16 * (255 - a) + f as u16 * a) / 255) as u8;
        Rgb {
            r: mix(self.r, fg.r),
            g: mix(self.g, fg.g),
            b: mix(self.b, fg.b),
        }
    }

    fn dim(self) -> Rgb {
        Rgb {
            r: (self.r as u16 * 2 / 3) as u8,
            g: (self.g as u16 * 2 / 3) as u8,
            b: (self.b as u16 * 2 / 3) as u8,
        }
    }
}

/// Farbschema im Umfang von iTerm2 (.itermcolors).
#[derive(Clone, Debug)]
pub struct Theme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    pub selection: Rgb,
    /// Textfarbe in der Markierung; `None` = Zellfarbe behalten
    pub selection_text: Option<Rgb>,
    pub ansi: [Rgb; 16],
}

/// Name des Standard-Schemas.
pub const DEFAULT_THEME: &str = "iTerm2 Default";

impl Default for Theme {
    fn default() -> Self {
        builtin_themes().remove(0).1
    }
}

fn palette(
    fg: u32,
    bg: u32,
    cursor: u32,
    cursor_text: u32,
    selection: u32,
    ansi: [u32; 16],
) -> Theme {
    Theme {
        foreground: Rgb::hex(fg),
        background: Rgb::hex(bg),
        cursor: Rgb::hex(cursor),
        cursor_text: Rgb::hex(cursor_text),
        selection: Rgb::hex(selection),
        selection_text: None,
        ansi: ansi.map(Rgb::hex),
    }
}

/// Mitgelieferte Schemas. Das erste ist der Standard.
pub fn builtin_themes() -> Vec<(&'static str, Theme)> {
    const ITERM: [u32; 16] = [
        0x000000, 0xc91b00, 0x00c200, 0xc7c400, 0x0225c7, 0xca30c7, 0x00c5c7, 0xc7c7c7, 0x686868,
        0xff6e67, 0x5ffa68, 0xfffc67, 0x6871ff, 0xff77ff, 0x60fdff, 0xffffff,
    ];
    const SOLARIZED: [u32; 16] = [
        0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5, 0x002b36,
        0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
    ];
    let mut iterm_dark = palette(0xc7c7c7, 0x000000, 0xc7c7c7, 0x000000, 0xc6dcfc, ITERM);
    iterm_dark.selection_text = Some(Rgb::hex(0x000000));
    let mut iterm_light = palette(0x000000, 0xffffff, 0x000000, 0xffffff, 0xc6dcfc, ITERM);
    iterm_light.selection_text = Some(Rgb::hex(0x000000));
    vec![
        (DEFAULT_THEME, iterm_dark),
        ("iTerm2 Light", iterm_light),
        (
            "Solarized Dark",
            palette(0x839496, 0x002b36, 0x839496, 0x002b36, 0x073642, SOLARIZED),
        ),
        (
            "Solarized Light",
            palette(0x657b83, 0xfdf6e3, 0x657b83, 0xfdf6e3, 0xeee8d5, SOLARIZED),
        ),
        (
            "Dracula",
            palette(
                0xf8f8f2,
                0x282a36,
                0xf8f8f2,
                0x282a36,
                0x44475a,
                [
                    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
                ],
            ),
        ),
        (
            "Gruvbox Dark",
            palette(
                0xebdbb2,
                0x282828,
                0xebdbb2,
                0x282828,
                0x504945,
                [
                    0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                    0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
                ],
            ),
        ),
        (
            "Nord",
            palette(
                0xd8dee9,
                0x2e3440,
                0xd8dee9,
                0x2e3440,
                0x434c5e,
                [
                    0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                    0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
                ],
            ),
        ),
        (
            "One Dark",
            palette(
                0xabb2bf,
                0x282c34,
                0x528bff,
                0x282c34,
                0x3e4451,
                [
                    0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
                    0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
                ],
            ),
        ),
        (
            "Tokyo Night",
            palette(
                0xc0caf5,
                0x1a1b26,
                0xc0caf5,
                0x1a1b26,
                0x33467c,
                [
                    0x15161e, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xa9b1d6,
                    0x414868, 0xf7768e, 0x9ece6a, 0xe0af68, 0x7aa2f7, 0xbb9af7, 0x7dcfff, 0xc0caf5,
                ],
            ),
        ),
        (
            "Catppuccin Mocha",
            palette(
                0xcdd6f4,
                0x1e1e2e,
                0xf5e0dc,
                0x1e1e2e,
                0x585b70,
                [
                    0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
                    0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
                ],
            ),
        ),
    ]
}

/// Ein Schema mit Namen; `path` ist gesetzt, wenn es aus einer Datei stammt.
#[derive(Clone, Debug)]
pub struct ThemeEntry {
    pub name: String,
    pub theme: Theme,
    pub path: Option<PathBuf>,
}

/// Alle verfügbaren Schemas: eingebaute + `~/.config/rterm2/themes/*.itermcolors`
/// (eine Datei mit gleichem Namen ersetzt das eingebaute Schema).
pub struct ThemeLibrary {
    pub entries: Vec<ThemeEntry>,
}

impl ThemeLibrary {
    pub fn dir() -> Option<PathBuf> {
        crate::config::app_dir().map(|d| d.join("themes"))
    }

    pub fn load() -> Self {
        let mut lib = Self {
            entries: builtin_themes()
                .into_iter()
                .map(|(name, theme)| ThemeEntry {
                    name: name.to_string(),
                    theme,
                    path: None,
                })
                .collect(),
        };
        if let Some(dir) = Self::dir() {
            if let Ok(read) = std::fs::read_dir(&dir) {
                for entry in read.flatten() {
                    let path = entry.path();
                    if path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("itermcolors"))
                    {
                        if let Err(e) = lib.add_file(&path) {
                            log::warn!("{e:#}");
                        }
                    }
                }
            }
        }
        lib.entries.sort_by_key(|e| e.name.to_lowercase());
        lib
    }

    /// Lädt eine Datei und fügt sie hinzu bzw. ersetzt ein Schema gleichen Namens.
    fn add_file(&mut self, path: &Path) -> anyhow::Result<usize> {
        let theme = Theme::load_itermcolors(path)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let entry = ThemeEntry {
            name: name.clone(),
            theme,
            path: Some(path.to_path_buf()),
        };
        Ok(match self.index_of(&name) {
            Some(i) => {
                self.entries[i] = entry;
                i
            }
            None => {
                self.entries.push(entry);
                self.entries.len() - 1
            }
        })
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub fn names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// `spec` aus der Config: Name eines Schemas oder Pfad zu einer .itermcolors-Datei.
    pub fn resolve(&mut self, spec: &str) -> anyhow::Result<usize> {
        let looks_like_path = spec.contains('/') || spec.to_lowercase().ends_with(".itermcolors");
        if looks_like_path {
            let path = match spec.strip_prefix("~/") {
                Some(rest) => dirs::home_dir()
                    .map(|h| h.join(rest))
                    .unwrap_or_else(|| PathBuf::from(spec)),
                None => PathBuf::from(spec),
            };
            return self.add_file(&path);
        }
        self.index_of(spec).ok_or_else(|| {
            anyhow!(
                "Farbschema „{spec}“ nicht gefunden. Vorhanden: {}",
                self.names().join(", ")
            )
        })
    }
}

impl Theme {
    /// Lädt eine iTerm2-Farbdatei, z. B. von https://iterm2colorschemes.com.
    /// Fehlende Einträge werden aus dem Standard-Schema übernommen.
    pub fn load_itermcolors(path: &Path) -> anyhow::Result<Theme> {
        let value = plist::Value::from_file(path).with_context(|| {
            format!("Farbschema {} konnte nicht gelesen werden", path.display())
        })?;
        let dict = value
            .as_dictionary()
            .ok_or_else(|| anyhow!("{}: kein Plist-Dictionary", path.display()))?;

        let get = |key: &str| -> Option<Rgb> {
            let entry = dict.get(key)?.as_dictionary()?;
            let comp = |name: &str| -> Option<u8> {
                let v = entry.get(name)?;
                let f = v
                    .as_real()
                    .or_else(|| v.as_signed_integer().map(|i| i as f64))?;
                Some((f.clamp(0.0, 1.0) * 255.0).round() as u8)
            };
            Some(Rgb {
                r: comp("Red Component")?,
                g: comp("Green Component")?,
                b: comp("Blue Component")?,
            })
        };

        let mut t = Theme::default();
        for i in 0..16 {
            if let Some(c) = get(&format!("Ansi {i} Color")) {
                t.ansi[i] = c;
            }
        }
        if let Some(c) = get("Foreground Color") {
            t.foreground = c;
        }
        if let Some(c) = get("Background Color") {
            t.background = c;
        }
        if let Some(c) = get("Cursor Color") {
            t.cursor = c;
        }
        if let Some(c) = get("Cursor Text Color") {
            t.cursor_text = c;
        }
        if let Some(c) = get("Selection Color") {
            t.selection = c;
        }
        t.selection_text = get("Selected Text Color");
        Ok(t)
    }

    pub fn resolve(&self, color: Color) -> Rgb {
        match color {
            Color::Spec(rgb) => Rgb {
                r: rgb.r,
                g: rgb.g,
                b: rgb.b,
            },
            Color::Indexed(i) => self.indexed(i),
            Color::Named(n) => self.named(n),
        }
    }

    fn named(&self, n: NamedColor) -> Rgb {
        match n {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::DimForeground => self.foreground.dim(),
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            _ => {
                let idx = n as usize;
                if idx < 16 {
                    self.ansi[idx]
                } else {
                    // DimBlack..DimWhite
                    let base = idx.saturating_sub(NamedColor::DimBlack as usize).min(7);
                    self.ansi[base].dim()
                }
            }
        }
    }

    fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
                let i = i - 16;
                Rgb {
                    r: STEPS[(i / 36) as usize],
                    g: STEPS[((i / 6) % 6) as usize],
                    b: STEPS[(i % 6) as usize],
                }
            }
            232..=255 => {
                let v = 8 + 10 * (i - 232);
                Rgb { r: v, g: v, b: v }
            }
        }
    }
}
