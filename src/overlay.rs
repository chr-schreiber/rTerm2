use crate::font::FontAtlas;
use crate::picker::ThemePicker;
use crate::renderer::{self, Canvas, Rect};
use crate::theme::{Theme, ThemeLibrary};

/// Maximal sichtbare Einträge in der Liste.
const MAX_ROWS: usize = 12;
/// Fußzeile – bestimmt auch die Mindestbreite, damit sie nie abgeschnitten wird.
const HINT: &str = "↑↓ wählen · tippen filtert · Enter übernehmen · Esc zurück";
/// Innenabstand des Panels
const PAD: usize = 8;

/// Zeichnet die Schema-Auswahl oben mittig ins Fenster. `ui` ist das gerade aktive (Vorschau-)Schema.
pub fn draw_theme_picker(
    frame: &mut dyn Canvas,
    area: Rect,
    font: &mut FontAtlas,
    ui: &Theme,
    lib: &ThemeLibrary,
    picker: &ThemePicker,
) {
    let (cw, ch) = (font.cell_w, font.cell_h);
    let names = lib.names();
    let visible = picker.visible(&names);
    let row_h = ch + 6;
    let rows = visible.len().clamp(1, MAX_ROWS);

    // Breit genug für die Fußzeile (+2 Zeichen Luft), aber nie breiter als das Fenster
    let wanted = (56 * cw).max((HINT.chars().count() + 2) * cw + 2 * PAD);
    let w = wanted
        .min(area.w.saturating_sub(40))
        .max(20 * cw)
        .min(area.w);
    // Kopfzeile + Trennlinie + Liste + Fußzeile, oben und unten gleicher Rand
    let h = 2 * PAD + row_h + 2 + rows * row_h + 2 + ch + 4;
    let x = area.x + area.w.saturating_sub(w) / 2;
    let y = area.y + (area.h / 8).min(60);
    let panel = Rect {
        x,
        y,
        w,
        h: h.min(area.h.saturating_sub(y - area.y)),
    };

    let bg = ui.background.blend(ui.foreground, 22);
    let border = ui.background.blend(ui.foreground, 110);
    let dim = ui.background.blend(ui.foreground, 150);
    frame.fill(area, panel.x, panel.y, panel.w, panel.h, bg);
    frame.fill(area, panel.x, panel.y, panel.w, 1, border);
    frame.fill(area, panel.x, panel.y + panel.h - 1, panel.w, 1, border);
    frame.fill(area, panel.x, panel.y, 1, panel.h, border);
    frame.fill(area, panel.x + panel.w - 1, panel.y, 1, panel.h, border);

    let inner = panel.inset(PAD);
    let text_y = |row: usize| inner.y + row * row_h + 3;

    // Kopfzeile mit Filter
    let header = format!("Farbschema: {}_", picker.filter);
    renderer::draw_text(
        frame,
        inner,
        font,
        &header,
        inner.x,
        text_y(0),
        ui.foreground,
    );
    let count = format!("{}/{}", visible.len(), names.len());
    let count_x = inner.x + inner.w.saturating_sub(count.chars().count() * cw);
    renderer::draw_text(frame, inner, font, &count, count_x, text_y(0), dim);
    frame.fill(inner, inner.x, inner.y + row_h, inner.w, 1, border);

    // Liste
    let list_top = inner.y + row_h + 2;
    if visible.is_empty() {
        renderer::draw_text(
            frame,
            inner,
            font,
            "keine Treffer",
            inner.x,
            list_top + 3,
            dim,
        );
    }
    let offset = picker.scroll_offset(rows);
    let swatch_w = cw.max(6);
    let swatches_w = 8 * swatch_w + 7 * 2;
    for (row, &idx) in visible.iter().skip(offset).take(rows).enumerate() {
        let entry = &lib.entries[idx];
        let ry = list_top + row * row_h;
        let selected = offset + row == picker.cursor;
        let fg = if selected {
            frame.fill(inner, inner.x, ry, inner.w, row_h, ui.selection);
            ui.selection_text.unwrap_or(ui.foreground)
        } else {
            ui.foreground
        };

        let mark = if idx == picker.original { "● " } else { "  " };
        let max_chars = inner.w.saturating_sub(swatches_w + 3 * cw) / cw;
        let label: String = format!("{mark}{}", entry.name)
            .chars()
            .take(max_chars)
            .collect();
        renderer::draw_text(frame, inner, font, &label, inner.x, ry + 3, fg);

        // Farbmuster des Schemas: Hintergrund als Rahmen, darin die 8 Grundfarben
        let sx = inner.x + inner.w.saturating_sub(swatches_w);
        let t = &entry.theme;
        frame.fill(
            inner,
            sx.saturating_sub(4),
            ry + 1,
            swatches_w + 8,
            row_h - 2,
            t.background,
        );
        for (i, c) in t.ansi[..8].iter().enumerate() {
            frame.fill(
                inner,
                sx + i * (swatch_w + 2),
                ry + 4,
                swatch_w,
                row_h.saturating_sub(8),
                *c,
            );
        }
    }

    // Fußzeile
    let hint_y = list_top + rows * row_h + 2;
    // Nur bei sehr schmalen Fenstern kürzen – dann sichtbar mit …
    let max = inner.w / cw.max(1);
    let hint: String = if HINT.chars().count() <= max {
        HINT.to_string()
    } else {
        HINT.chars()
            .take(max.saturating_sub(1))
            .chain(['…'])
            .collect()
    };
    renderer::draw_text(
        frame,
        inner,
        font,
        &hint,
        inner.x,
        hint_y.min(inner.y + inner.h.saturating_sub(ch)),
        dim,
    );
}
