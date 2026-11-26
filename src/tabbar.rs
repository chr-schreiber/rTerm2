use crate::font::FontAtlas;
use crate::renderer::{self, Canvas, Rect};
use crate::theme::Theme;

/// Höhe der Tab-Leiste in Pixeln.
pub fn height(font: &FontAtlas) -> usize {
    font.cell_h + 12
}

/// Tabs teilen sich die Breite gleichmäßig (wie in iTerm2).
fn tab_rect(bar: Rect, n: usize, i: usize) -> Rect {
    let tw = bar.w / n.max(1);
    let x = bar.x + i * tw;
    let w = if i + 1 == n { bar.w - i * tw } else { tw };
    Rect {
        x,
        y: bar.y,
        w,
        h: bar.h,
    }
}

/// Welcher Tab liegt unter der x-Koordinate?
pub fn hit(bar: Rect, n: usize, x: f64, y: f64) -> Option<usize> {
    if !bar.contains(x, y) {
        return None;
    }
    (0..n).find(|&i| tab_rect(bar, n, i).contains(x, y))
}

pub fn draw(
    frame: &mut dyn Canvas,
    bar: Rect,
    titles: &[String],
    active: usize,
    font: &mut FontAtlas,
    theme: &Theme,
) {
    let bar_bg = theme.background.blend(theme.foreground, 28);
    let separator = theme.background.blend(theme.foreground, 70);
    let inactive_fg = theme.background.blend(theme.foreground, 140);
    frame.fill(bar, bar.x, bar.y, bar.w, bar.h, bar_bg);

    let n = titles.len();
    for (i, title) in titles.iter().enumerate() {
        let r = tab_rect(bar, n, i);
        let fg = if i == active {
            // Aktiver Tab geht nahtlos in den Terminalbereich über.
            frame.fill(r, r.x, r.y, r.w, r.h, theme.background);
            theme.foreground
        } else {
            frame.fill(bar, r.x, r.y + r.h - 1, r.w, 1, separator);
            inactive_fg
        };
        if i > 0 {
            frame.fill(bar, r.x, r.y, 1, r.h, separator);
        }

        let max_chars = r.w.saturating_sub(16) / font.cell_w;
        let label = truncate(&format!("{}  {}", i + 1, title), max_chars);
        let text_w = label.chars().count() * font.cell_w;
        let tx = r.x + r.w.saturating_sub(text_w) / 2;
        let ty = r.y + (r.h.saturating_sub(font.cell_h)) / 2;
        renderer::draw_text(frame, r, font, &label, tx, ty, fg);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_and_hit() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("abc", 0), "");
        let bar = Rect {
            x: 0,
            y: 0,
            w: 301,
            h: 20,
        };
        assert_eq!(hit(bar, 3, 5.0, 5.0), Some(0));
        assert_eq!(hit(bar, 3, 300.0, 5.0), Some(2));
        assert_eq!(hit(bar, 3, 150.0, 25.0), None);
    }
}
