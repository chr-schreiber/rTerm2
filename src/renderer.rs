use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::Match;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::CursorShape;

use crate::font::FontAtlas;
use crate::theme::{Rgb, Theme};

/// Rechteck in Pixeln innerhalb des Framebuffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    /// Verkleinert das Rechteck auf allen Seiten um `p` Pixel.
    pub fn inset(self, p: usize) -> Rect {
        Rect {
            x: self.x + p,
            y: self.y + p,
            w: self.w.saturating_sub(2 * p),
            h: self.h.saturating_sub(2 * p),
        }
    }

    /// Vergrößert das Rechteck auf allen Seiten um `p` Pixel (Trefferzone für Trennlinien).
    pub fn grow(self, p: usize) -> Rect {
        Rect {
            x: self.x.saturating_sub(p),
            y: self.y.saturating_sub(p),
            w: self.w + 2 * p,
            h: self.h + 2 * p,
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < (self.x + self.w) as f64
            && y < (self.y + self.h) as f64
    }
}

/// Zeichenfläche, auf die Terminal, Tab-Leiste und Overlays zeichnen.
/// Zwei Umsetzungen: [`Frame`] (CPU-Framebuffer) und `gpu::GpuCanvas` (wgpu).
/// Reihenfolge der Aufrufe = Zeichenreihenfolge (Späteres liegt oben).
pub trait Canvas {
    /// Ganze Fläche mit einer Farbe füllen.
    fn clear(&mut self, color: Rgb);
    /// Rechteck füllen, auf `clip` beschnitten.
    fn fill(&mut self, clip: Rect, x: usize, y: usize, w: usize, h: usize, color: Rgb);
    /// `color` mit Deckkraft `alpha` (0..=255) über ein Rechteck legen.
    fn tint(&mut self, r: Rect, color: Rgb, alpha: u8);
    /// Ein Zeichen in die Zelle mit linker oberer Ecke (x, y) setzen, auf `clip` beschnitten.
    fn glyph(&mut self, clip: Rect, font: &mut FontAtlas, c: char, x: usize, y: usize, color: Rgb);
}

/// Framebuffer (0x00RRGGBB pro Pixel), Zeile für Zeile.
pub struct Frame<'a> {
    pub buf: &'a mut [u32],
    pub width: usize,
    pub height: usize,
}

impl Canvas for Frame<'_> {
    fn clear(&mut self, color: Rgb) {
        self.buf.fill(color.to_u32());
    }

    fn fill(&mut self, clip: Rect, x: usize, y: usize, w: usize, h: usize, color: Rgb) {
        let x0 = x.max(clip.x);
        let y0 = y.max(clip.y);
        let x1 = (x + w).min(clip.x + clip.w).min(self.width);
        let y1 = (y + h).min(clip.y + clip.h).min(self.height);
        let c = color.to_u32();
        for yy in y0..y1 {
            let row = yy * self.width;
            self.buf[row + x0..row + x1.max(x0)].fill(c);
        }
    }

    fn tint(&mut self, r: Rect, color: Rgb, alpha: u8) {
        let x1 = (r.x + r.w).min(self.width);
        let y1 = (r.y + r.h).min(self.height);
        for yy in r.y..y1 {
            let row = yy * self.width;
            for px in &mut self.buf[row + r.x.min(x1)..row + x1] {
                *px = Rgb::from_u32(*px).blend(color, alpha).to_u32();
            }
        }
    }

    fn glyph(&mut self, clip: Rect, font: &mut FontAtlas, c: char, x: usize, y: usize, color: Rgb) {
        let ascent = font.ascent;
        let g = font.glyph(c);
        let m = g.metrics;
        let baseline = y as isize + ascent.round() as isize;
        let gx = x as isize + m.xmin as isize;
        let gy = baseline - (m.height as isize + m.ymin as isize);
        for row in 0..m.height {
            for col in 0..m.width {
                let a = g.coverage[row * m.width + col];
                if a > 0 {
                    blend_px(self, clip, gx + col as isize, gy + row as isize, color, a);
                }
            }
        }
    }
}

fn blend_px(frame: &mut Frame, clip: Rect, x: isize, y: isize, color: Rgb, alpha: u8) {
    if x < clip.x as isize || y < clip.y as isize {
        return;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= (clip.x + clip.w).min(frame.width) || y >= (clip.y + clip.h).min(frame.height) {
        return;
    }
    let i = y * frame.width + x;
    frame.buf[i] = Rgb::from_u32(frame.buf[i]).blend(color, alpha).to_u32();
}

/// Farben der Suchtreffer (wie iTerm2: gelb, der aktuelle orange) – unabhängig vom Schema gut sichtbar.
const MATCH_BG: Rgb = Rgb::hex(0xffe066);
const CURRENT_MATCH_BG: Rgb = Rgb::hex(0xff9632);
const MATCH_FG: Rgb = Rgb::hex(0x000000);

/// Suchtreffer, die in einer Pane hervorgehoben werden.
#[derive(Default)]
pub struct Highlights<'a> {
    pub marks: &'a [Match],
    pub current: Option<&'a Match>,
}

/// Zeichnet den sichtbaren Bereich eines Terminals in `rect`.
pub fn draw_terminal<T: EventListener>(
    frame: &mut dyn Canvas,
    rect: Rect,
    term: &Term<T>,
    font: &mut FontAtlas,
    theme: &Theme,
    focused: bool,
    highlights: &Highlights,
) {
    let (cw, ch) = (font.cell_w, font.cell_h);
    let screen_lines = term.screen_lines() as i32;
    let content = term.renderable_content();
    let offset = content.display_offset as i32;

    for indexed in content.display_iter {
        let row = indexed.point.line.0 + offset;
        if row < 0 || row >= screen_lines {
            continue;
        }
        let cell = indexed.cell;
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }

        let mut fg = theme.resolve(cell.fg);
        let mut bg = theme.resolve(cell.bg);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        let point = indexed.point;
        if highlights.current.is_some_and(|m| m.contains(&point)) {
            bg = CURRENT_MATCH_BG;
            fg = MATCH_FG;
        } else if highlights.marks.iter().any(|m| m.contains(&point)) {
            bg = MATCH_BG;
            fg = MATCH_FG;
        }
        let selected = content.selection.as_ref().is_some_and(|sel| {
            sel.contains_cell(&indexed, content.cursor.point, content.cursor.shape)
        });
        if selected {
            bg = theme.selection;
            fg = theme.selection_text.unwrap_or(fg);
        }

        let x = rect.x + indexed.point.column.0 * cw;
        let y = rect.y + row as usize * ch;
        let w = if cell.flags.contains(Flags::WIDE_CHAR) {
            2 * cw
        } else {
            cw
        };

        if bg != theme.background {
            frame.fill(rect, x, y, w, ch, bg);
        }
        if cell.c != ' ' && cell.c != '\0' && !cell.flags.contains(Flags::HIDDEN) {
            frame.glyph(rect, font, cell.c, x, y, fg);
        }
        if cell.flags.contains(Flags::UNDERLINE) {
            frame.fill(rect, x, y + ch - 2, w, 1, fg);
        }
        if cell.flags.contains(Flags::STRIKEOUT) {
            frame.fill(rect, x, y + ch / 2, w, 1, fg);
        }
    }

    // Cursor
    let mut cursor = content.cursor;
    // Inaktive Panes zeigen den Cursor nur als Rahmen (wie iTerm2).
    if !focused && cursor.shape == CursorShape::Block {
        cursor.shape = CursorShape::HollowBlock;
    }
    let row = cursor.point.line.0 + offset;
    if cursor.shape != CursorShape::Hidden && row >= 0 && row < screen_lines {
        let x = rect.x + cursor.point.column.0 * cw;
        let y = rect.y + row as usize * ch;
        match cursor.shape {
            CursorShape::Block => {
                frame.fill(rect, x, y, cw, ch, theme.cursor);
                let c = term.grid()[cursor.point].c;
                if c != ' ' && c != '\0' {
                    frame.glyph(rect, font, c, x, y, theme.cursor_text);
                }
            }
            CursorShape::Beam => frame.fill(rect, x, y, 2, ch, theme.cursor),
            CursorShape::Underline => frame.fill(rect, x, y + ch - 2, cw, 2, theme.cursor),
            CursorShape::HollowBlock => {
                frame.fill(rect, x, y, cw, 1, theme.cursor);
                frame.fill(rect, x, y + ch - 1, cw, 1, theme.cursor);
                frame.fill(rect, x, y, 1, ch, theme.cursor);
                frame.fill(rect, x + cw - 1, y, 1, ch, theme.cursor);
            }
            CursorShape::Hidden => {}
        }
    }
}

/// Einzeilige Beschriftung (Tab-Leiste u. Ä.), monospaced.
pub fn draw_text(
    frame: &mut dyn Canvas,
    clip: Rect,
    font: &mut FontAtlas,
    text: &str,
    x: usize,
    y: usize,
    color: Rgb,
) {
    let mut cx = x;
    for c in text.chars() {
        if c != ' ' {
            frame.glyph(clip, font, c, cx, y, color);
        }
        cx += font.cell_w;
    }
}
