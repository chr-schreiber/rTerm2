use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::term::TermMode;
use winit::keyboard::ModifiersState;

use crate::renderer::Rect;

/// Xterm-Codes der Maustasten.
pub const BTN_LEFT: u8 = 0;
pub const BTN_MIDDLE: u8 = 1;
pub const BTN_RIGHT: u8 = 2;
pub const WHEEL_UP: u8 = 64;
pub const WHEEL_DOWN: u8 = 65;
/// Wird bei Bewegung mit gedrückter Taste addiert.
pub const MOTION: u8 = 32;

/// Zelle (Spalte, Zeile im sichtbaren Bereich) unter einem Pixel; auf das Grid begrenzt.
pub fn cell_at(
    inner: Rect,
    cell_w: usize,
    cell_h: usize,
    cols: usize,
    lines: usize,
    x: f64,
    y: f64,
) -> (usize, usize, Side) {
    let rx = (x - inner.x as f64).max(0.0);
    let ry = (y - inner.y as f64).max(0.0);
    let col = ((rx / cell_w as f64) as usize).min(cols.saturating_sub(1));
    let row = ((ry / cell_h as f64) as usize).min(lines.saturating_sub(1));
    let cw = cell_w as f64;
    let side = if rx % cw < cw / 2.0 {
        Side::Left
    } else {
        Side::Right
    };
    (col, row, side)
}

/// Grid-Punkt für eine Zelle im sichtbaren Bereich (berücksichtigt den Scrollback-Offset).
pub fn grid_point(col: usize, row: usize, display_offset: usize) -> Point {
    Point::new(Line(row as i32 - display_offset as i32), Column(col))
}

/// Ob das Programm im Terminal Mausereignisse haben will. Mit Shift nimmt rterm2 die Maus selbst
/// (Text markieren in mc, htop, vim …), wie in iTerm2 mit ⌥.
pub fn reporting(mode: TermMode, mods: ModifiersState) -> bool {
    mode.intersects(TermMode::MOUSE_MODE) && !mods.shift_key()
}

/// Ob Bewegungen mit gedrückter Taste gemeldet werden sollen.
pub fn reports_motion(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
}

/// Kodiert ein Mausereignis für das PTY (SGR 1006 oder klassisches X10/1000-Format).
pub fn encode(
    mode: TermMode,
    button: u8,
    col: usize,
    row: usize,
    pressed: bool,
    mods: ModifiersState,
) -> Vec<u8> {
    let mut b = button;
    if mods.shift_key() {
        b += 4;
    }
    if mods.alt_key() {
        b += 8;
    }
    if mods.control_key() {
        b += 16;
    }
    if mode.contains(TermMode::SGR_MOUSE) {
        let end = if pressed { 'M' } else { 'm' };
        return format!("\x1b[<{b};{};{}{end}", col + 1, row + 1).into_bytes();
    }
    // Klassisch: Loslassen = Taste 3, Koordinaten max. 223
    let cb = if pressed { b } else { 3 | (b & 0b1_1100) };
    if col > 222 || row > 222 {
        return Vec::new();
    }
    vec![0x1b, b'[', b'M', 32 + cb, 33 + col as u8, 33 + row as u8]
}

/// Bereitet Text aus der Zwischenablage für das PTY auf.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // ESC entfernen, damit der Text die Klammerung nicht vorzeitig beenden kann.
        let clean: String = text.chars().filter(|&c| c != '\x1b').collect();
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}
