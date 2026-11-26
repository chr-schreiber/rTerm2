//! Copy-Mode (wie tmux/vim): Cursor per Tastatur bewegen, markieren, kopieren, suchen.
//! Baut auf dem vi-Modus von `alacritty_terminal` auf (Bewegungen, Markierung, Regex-Suche).

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vi_mode::ViMotion;
use winit::event::KeyEvent;
use winit::keyboard::{Key, NamedKey};

use crate::pane::PaneId;

/// Höchstens so viele Treffer werden im sichtbaren Bereich hervorgehoben.
const MAX_MARKS: usize = 500;

/// Eingabezeile der Suche.
pub struct SearchPrompt {
    pub query: String,
    /// Suche nach oben (`?`, Ctrl+Shift+F) statt nach unten (`/`)
    pub backward: bool,
    /// Cursorposition beim Öffnen – Esc kehrt dorthin zurück
    origin: Point,
}

pub struct CopyState {
    pub pane: PaneId,
    pub prompt: Option<SearchPrompt>,
    regex: Option<RegexSearch>,
    backward: bool,
    pub current: Option<Match>,
    /// Erstes `g` von `gg` gedrückt
    pending_g: bool,
}

/// Was nach einer Taste passieren soll.
pub enum Outcome {
    Stay,
    Exit,
    /// Text kopieren, dann beenden
    Yank(String),
}

/// Sucht wörtlich: Regex-Sonderzeichen werden maskiert. Groß/Klein: „smart case“ macht
/// `RegexSearch` selbst (nur Kleinbuchstaben → unabhängig von Groß/Klein).
pub fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl CopyState {
    /// Copy-Mode für eine Pane einschalten; mit `search` gleich die Suche nach oben öffnen.
    pub fn enter<T: EventListener>(pane: PaneId, term: &mut Term<T>, search: bool) -> Self {
        if !term.mode().contains(TermMode::VI) {
            term.toggle_vi_mode();
        }
        let origin = term.vi_mode_cursor.point;
        Self {
            pane,
            prompt: search.then(|| SearchPrompt {
                query: String::new(),
                backward: true,
                origin,
            }),
            regex: None,
            backward: true,
            current: None,
            pending_g: false,
        }
    }

    /// Copy-Mode verlassen: vi-Modus aus, Markierung weg, zurück ans Ende.
    pub fn leave<T: EventListener>(term: &mut Term<T>) {
        if term.mode().contains(TermMode::VI) {
            term.toggle_vi_mode();
        }
        term.selection = None;
        term.scroll_display(Scroll::Bottom);
    }

    /// Treffer im sichtbaren Bereich (zum Hervorheben).
    pub fn visible_matches<T: EventListener>(&mut self, term: &Term<T>) -> Vec<Match> {
        let Some(regex) = self.regex.as_mut() else {
            return Vec::new();
        };
        let offset = term.grid().display_offset() as i32;
        let start = Point::new(Line(-offset), Column(0));
        let end = Point::new(
            Line(term.screen_lines() as i32 - 1 - offset),
            Column(term.columns().saturating_sub(1)),
        );
        RegexIter::new(start, end, Direction::Right, term, regex)
            .take(MAX_MARKS)
            .collect()
    }

    pub fn handle_key<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        event: &KeyEvent,
        ctrl: bool,
        shift: bool,
    ) -> Outcome {
        if self.prompt.is_some() {
            self.prompt_key(term, event, ctrl);
            return Outcome::Stay;
        }
        let g_was_pending = std::mem::take(&mut self.pending_g);
        // Vorab lesen: `move_lines(term, term.…)` ginge am Borrow-Checker nicht vorbei
        let page = term.screen_lines() as i32;
        let half = page / 2;
        let (top, bottom) = (term.topmost_line(), term.bottommost_line());

        // Benannte Tasten
        if let Key::Named(n) = &event.logical_key {
            match n {
                NamedKey::Escape => return self.escape(term),
                NamedKey::Enter => return self.yank(term),
                NamedKey::Space if ctrl && shift => return Outcome::Exit,
                NamedKey::ArrowLeft => term.vi_motion(ViMotion::Left),
                NamedKey::ArrowRight => term.vi_motion(ViMotion::Right),
                NamedKey::ArrowUp => term.vi_motion(ViMotion::Up),
                NamedKey::ArrowDown => term.vi_motion(ViMotion::Down),
                NamedKey::Home => term.vi_motion(ViMotion::First),
                NamedKey::End => term.vi_motion(ViMotion::Last),
                NamedKey::PageUp => move_lines(term, -page),
                NamedKey::PageDown => move_lines(term, page),
                _ => {}
            }
            return Outcome::Stay;
        }

        // Ctrl+Buchstabe
        if ctrl {
            let Key::Character(c) = &event.logical_key else {
                return Outcome::Stay;
            };
            match c.to_lowercase().as_str() {
                "u" => move_lines(term, -half),
                "d" => move_lines(term, half),
                "b" => move_lines(term, -page),
                "f" => move_lines(term, page),
                "v" => toggle_selection(term, SelectionType::Block),
                "c" => return Outcome::Exit,
                _ => {}
            }
            return Outcome::Stay;
        }

        // Zeichen wie getippt (Shift/Layout berücksichtigt: G, $, { …)
        let Some(text) = event.text.as_deref() else {
            return Outcome::Stay;
        };
        match text {
            "h" => term.vi_motion(ViMotion::Left),
            "j" => term.vi_motion(ViMotion::Down),
            "k" => term.vi_motion(ViMotion::Up),
            "l" => term.vi_motion(ViMotion::Right),
            "w" => term.vi_motion(ViMotion::SemanticRight),
            "b" => term.vi_motion(ViMotion::SemanticLeft),
            "e" => term.vi_motion(ViMotion::SemanticRightEnd),
            "W" => term.vi_motion(ViMotion::WordRight),
            "B" => term.vi_motion(ViMotion::WordLeft),
            "E" => term.vi_motion(ViMotion::WordRightEnd),
            "0" => term.vi_motion(ViMotion::First),
            "$" => term.vi_motion(ViMotion::Last),
            "^" => term.vi_motion(ViMotion::FirstOccupied),
            "H" => term.vi_motion(ViMotion::High),
            "M" => term.vi_motion(ViMotion::Middle),
            "L" => term.vi_motion(ViMotion::Low),
            "{" => term.vi_motion(ViMotion::ParagraphUp),
            "}" => term.vi_motion(ViMotion::ParagraphDown),
            "%" => term.vi_motion(ViMotion::Bracket),
            "g" if g_was_pending => jump_to_line(term, top),
            "g" => self.pending_g = true,
            "G" => jump_to_line(term, bottom),
            "v" => toggle_selection(term, SelectionType::Simple),
            "V" => toggle_selection(term, SelectionType::Lines),
            "y" => return self.yank(term),
            "q" => return Outcome::Exit,
            "/" | "?" => {
                self.prompt = Some(SearchPrompt {
                    query: String::new(),
                    backward: text == "?",
                    origin: term.vi_mode_cursor.point,
                });
            }
            "n" => self.search_again(term, self.backward),
            "N" => self.search_again(term, !self.backward),
            _ => {}
        }
        Outcome::Stay
    }

    fn escape<T: EventListener>(&mut self, term: &mut Term<T>) -> Outcome {
        if term.selection.is_some() {
            term.selection = None;
            Outcome::Stay
        } else {
            Outcome::Exit
        }
    }

    fn yank<T: EventListener>(&mut self, term: &mut Term<T>) -> Outcome {
        match term.selection_to_string() {
            Some(text) if !text.is_empty() => Outcome::Yank(text),
            _ => Outcome::Stay,
        }
    }

    fn prompt_key<T: EventListener>(&mut self, term: &mut Term<T>, event: &KeyEvent, ctrl: bool) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let mut changed = false;
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                let origin = prompt.origin;
                self.prompt = None;
                self.current = None;
                term.vi_mode_cursor.point = origin;
                term.scroll_to_point(origin);
                return;
            }
            // Enter: Suche bestätigen, Cursor bleibt auf dem Treffer, weiter mit n/N
            Key::Named(NamedKey::Enter) => {
                self.prompt = None;
                return;
            }
            Key::Named(NamedKey::Backspace) => {
                prompt.query.pop();
                changed = true;
            }
            _ if !ctrl => {
                if let Some(text) = &event.text {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        prompt.query.push(c);
                        changed = true;
                    }
                }
            }
            _ => {}
        }
        if !changed {
            return;
        }

        // Live-Suche ab der Startposition
        let (query, backward, origin) = (prompt.query.clone(), prompt.backward, prompt.origin);
        self.backward = backward;
        self.regex = if query.is_empty() {
            None
        } else {
            RegexSearch::new(&escape_literal(&query)).ok()
        };
        self.current = match self.regex.as_mut() {
            Some(regex) => search(term, regex, origin, backward, false),
            None => None,
        };
        if self.current.is_none() {
            term.vi_mode_cursor.point = origin;
            term.scroll_to_point(origin);
        }
    }

    fn search_again<T: EventListener>(&mut self, term: &mut Term<T>, backward: bool) {
        let Some(regex) = self.regex.as_mut() else {
            return;
        };
        let from = self
            .current
            .as_ref()
            .map(|m| *m.start())
            .unwrap_or(term.vi_mode_cursor.point);
        if let Some(m) = search(term, regex, from, backward, true) {
            self.current = Some(m);
        }
    }

    /// Kein Treffer für die aktuelle Eingabe?
    pub fn no_match(&self) -> bool {
        self.prompt.as_ref().is_some_and(|p| !p.query.is_empty()) && self.current.is_none()
    }
}

/// Nächster Treffer ab `from`; `skip` überspringt den Treffer an `from` selbst (für n/N).
/// Setzt den Copy-Cursor auf den Trefferanfang und scrollt ihn in Sicht.
fn search<T: EventListener>(
    term: &mut Term<T>,
    regex: &mut RegexSearch,
    from: Point,
    backward: bool,
    skip: bool,
) -> Option<Match> {
    let (direction, side) = if backward {
        (Direction::Left, Side::Right)
    } else {
        (Direction::Right, Side::Left)
    };
    let origin = match (skip, backward) {
        (false, _) => from,
        (true, true) => from.sub(&*term, Boundary::None, 1),
        (true, false) => from.add(&*term, Boundary::None, 1),
    };
    let m = term.search_next(regex, origin, direction, side, None)?;
    term.vi_mode_cursor.point = *m.start();
    term.scroll_to_point(*m.start());
    Some(m)
}

/// Cursor um `n` Zeilen bewegen (negativ = nach oben); die Markierung wächst mit.
fn move_lines<T: EventListener>(term: &mut Term<T>, n: i32) {
    let motion = if n < 0 { ViMotion::Up } else { ViMotion::Down };
    for _ in 0..n.unsigned_abs() {
        term.vi_motion(motion);
    }
}

fn jump_to_line<T: EventListener>(term: &mut Term<T>, line: Line) {
    term.vi_mode_cursor.point = Point::new(line, Column(0));
    // vi_motion scrollt in Sicht und zieht eine Markierung nach
    term.vi_motion(ViMotion::First);
}

/// `v`/`V`/Ctrl+V: Markierung dieser Art beginnen oder wieder aufheben.
fn toggle_selection<T: EventListener>(term: &mut Term<T>, ty: SelectionType) {
    if term.selection.as_ref().is_some_and(|s| s.ty == ty) {
        term.selection = None;
        return;
    }
    let mut sel = Selection::new(ty, term.vi_mode_cursor.point, Side::Left);
    sel.include_all();
    term.selection = Some(sel);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal() {
        assert_eq!(escape_literal("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape_literal("fn main()"), "fn main\\(\\)");
        assert_eq!(escape_literal("~/x-y"), "~/x-y");
    }
}
