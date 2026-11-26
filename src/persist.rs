//! Schreibt Einstellungen zurück in die config.toml, ohne Kommentare oder Formatierung des Nutzers zu verlieren.

use std::path::Path;

/// Ersetzt bzw. ergänzt die Zeile `theme = "…"` auf oberster Ebene (vor der ersten `[Sektion]`).
pub fn set_theme_line(text: &str, name: &str) -> String {
    let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
    let new_line = format!("theme = \"{escaped}\"");

    let mut out: Vec<String> = Vec::new();
    let mut replaced = false;
    let mut in_top_level = true;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_top_level = false;
        }
        let is_theme_key = trimmed
            .strip_prefix("theme")
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if in_top_level && is_theme_key && !replaced {
            out.push(new_line.clone());
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.insert(0, new_line);
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// Speichert den Schemanamen in der Config-Datei (legt sie bei Bedarf an).
pub fn save_theme(config_path: &Path, name: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(config_path).unwrap_or_default();
    if let Some(dir) = config_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(config_path, set_theme_line(&text, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_line() {
        let cfg = "# meine Config\ntheme = \"Nord\"  \npadding = 4\n\n[font]\nsize = 13.0\n";
        assert_eq!(
            set_theme_line(cfg, "Dracula"),
            "# meine Config\ntheme = \"Dracula\"\npadding = 4\n\n[font]\nsize = 13.0\n"
        );
    }

    #[test]
    fn inserts_on_top_and_ignores_sections() {
        let cfg = "[font]\ntheme = \"nicht hier\"\n";
        assert_eq!(
            set_theme_line(cfg, "Nord"),
            "theme = \"Nord\"\n[font]\ntheme = \"nicht hier\"\n"
        );
        assert_eq!(set_theme_line("", "Nord"), "theme = \"Nord\"\n");
        assert!(set_theme_line("themes_dir = 1\n", "X").starts_with("theme = \"X\"\nthemes_dir"));
    }

    #[test]
    fn escapes_quotes_and_writes_file() {
        assert_eq!(set_theme_line("", "A \"B\""), "theme = \"A \\\"B\\\"\"\n");
        let dir = std::env::temp_dir().join(format!("rterm2-test-{}", std::process::id()));
        let path = dir.join("sub").join("config.toml");
        save_theme(&path, "Nord").unwrap();
        save_theme(&path, "Dracula").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme = \"Dracula\"\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
