use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

/// Konfiguration aus `~/.config/rterm2/config.toml`. Alle Felder sind optional.
///
/// ```toml
/// theme = "Dracula"            # eingebautes Schema, Datei in ~/.config/rterm2/themes/ oder Pfad
/// shell = "/usr/bin/fish"
///
/// [font]
/// path = "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf"
/// size = 14.0
///
/// [keybindings]                # ersetzt die Standard-Kürzel der genannten Aktionen
/// split_right = "super+d"
/// focus_left = ["ctrl+shift+h", "alt+left"]
/// close_pane = "none"          # abschalten
/// ```
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    /// Name eines Schemas oder Pfad zu einer .itermcolors-Datei
    pub theme: Option<String>,
    pub shell: Option<String>,
    pub padding: u32,
    /// "auto" (GPU, sonst CPU), "gpu" oder "cpu"
    pub renderer: String,
    /// Aktion → ein Kürzel oder eine Liste von Kürzeln
    pub keybindings: HashMap<String, OneOrMany>,
}

/// `"ctrl+d"` oder `["ctrl+d", "super+d"]`
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl Config {
    /// `[keybindings]` als Aktion → Liste von Kürzeln.
    pub fn keybindings(&self) -> HashMap<String, Vec<String>> {
        self.keybindings
            .iter()
            .map(|(k, v)| {
                let specs = match v {
                    OneOrMany::One(s) => vec![s.clone()],
                    OneOrMany::Many(v) => v.clone(),
                };
                (k.clone(), specs)
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    /// Pfad zu einer TTF/OTF-Datei. Ohne Angabe wird `fc-match monospace` gefragt.
    pub path: Option<PathBuf>,
    /// Größe in Punkt (wird mit dem Skalierungsfaktor des Monitors multipliziert).
    pub size: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            theme: None,
            shell: None,
            padding: 6,
            renderer: "auto".into(),
            keybindings: HashMap::new(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            path: None,
            size: 14.0,
        }
    }
}

/// Programmname: Fenstertitel, Wayland-App-ID / X11-Klasse, Config-Ordner.
pub const APP_NAME: &str = "rterm2";

/// `~/.config/rterm2`
pub fn app_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_NAME))
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        app_dir().map(|d| d.join("config.toml"))
    }

    pub fn load() -> anyhow::Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let mut cfg: Config = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Fehler in {}: {e}", path.display()))?;
        cfg.font.path = cfg.font.path.map(expand_tilde);
        Ok(cfg)
    }
}

fn expand_tilde(p: PathBuf) -> PathBuf {
    match p.strip_prefix("~") {
        Ok(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or(p),
        Err(_) => p,
    }
}
