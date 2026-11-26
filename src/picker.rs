//! Zustand der Schema-Auswahl (Ctrl+Shift+P): Filter, Cursor, Ursprungsschema zum Zurücksetzen.
//! Bewusst ohne Abhängigkeiten, damit die Logik unit-testbar ist.

/// Passt `filter` als Teilfolge (Groß/Klein egal) in `name`? „tn“ findet „Tokyo Night“.
pub fn fuzzy_match(name: &str, filter: &str) -> bool {
    let mut chars = name.chars().flat_map(char::to_lowercase);
    filter
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .all(|f| chars.any(|c| c == f))
}

pub struct ThemePicker {
    pub filter: String,
    /// Position in der gefilterten Liste
    pub cursor: usize,
    /// Schema beim Öffnen (Index in der Bibliothek) – Esc kehrt dorthin zurück
    pub original: usize,
}

impl ThemePicker {
    pub fn new(names: &[&str], current: usize) -> Self {
        let mut p = Self {
            filter: String::new(),
            cursor: 0,
            original: current,
        };
        p.cursor = p
            .visible(names)
            .iter()
            .position(|&i| i == current)
            .unwrap_or(0);
        p
    }

    /// Indizes (in der Bibliothek) der Schemas, die zum Filter passen.
    pub fn visible(&self, names: &[&str]) -> Vec<usize> {
        (0..names.len())
            .filter(|&i| fuzzy_match(names[i], &self.filter))
            .collect()
    }

    /// Aktuell markiertes Schema (Index in der Bibliothek).
    pub fn selected(&self, names: &[&str]) -> Option<usize> {
        self.visible(names).get(self.cursor).copied()
    }

    pub fn move_by(&mut self, names: &[&str], delta: isize) {
        let len = self.visible(names).len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    pub fn push(&mut self, c: char) {
        self.filter.push(c);
        self.cursor = 0;
    }

    pub fn pop(&mut self) {
        self.filter.pop();
        self.cursor = 0;
    }

    /// Erste sichtbare Zeile, damit der Cursor in einem Fenster von `rows` Zeilen sichtbar bleibt.
    pub fn scroll_offset(&self, rows: usize) -> usize {
        if rows == 0 {
            return 0;
        }
        self.cursor.saturating_sub(rows - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 5] = [
        "Dracula",
        "Gruvbox Dark",
        "iTerm2 Default",
        "Nord",
        "Tokyo Night",
    ];

    #[test]
    fn fuzzy() {
        assert!(fuzzy_match("Tokyo Night", "tn"));
        assert!(fuzzy_match("Tokyo Night", "TOKYO"));
        assert!(fuzzy_match("Gruvbox Dark", "gruv dark"));
        assert!(!fuzzy_match("Nord", "nt"));
        assert!(fuzzy_match("Nord", ""));
    }

    #[test]
    fn navigate_and_filter() {
        let mut p = ThemePicker::new(&NAMES, 2);
        assert_eq!(p.selected(&NAMES), Some(2));
        p.move_by(&NAMES, 10);
        assert_eq!(p.selected(&NAMES), Some(4));
        p.move_by(&NAMES, -10);
        assert_eq!(p.selected(&NAMES), Some(0));

        p.push('d');
        p.push('a');
        // „da“: Dracula, Gruvbox Dark, iTerm2 Default
        assert_eq!(p.visible(&NAMES), vec![0, 1, 2]);
        p.move_by(&NAMES, 1);
        assert_eq!(p.selected(&NAMES), Some(1));
        p.push('x');
        assert_eq!(p.selected(&NAMES), None);
        p.pop();
        assert_eq!(p.selected(&NAMES), Some(0));
        assert_eq!(p.original, 2);
    }

    #[test]
    fn scrolling() {
        let mut p = ThemePicker::new(&NAMES, 0);
        assert_eq!(p.scroll_offset(3), 0);
        p.move_by(&NAMES, 4);
        assert_eq!(p.scroll_offset(3), 2);
    }
}
