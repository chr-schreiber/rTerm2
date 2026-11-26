//! Geometrie für den GPU-Renderer: Rechtecke beschneiden und Glyphen in den Atlas packen.
//! Ohne Abhängigkeiten, damit unit-testbar.

/// Rechteck `[x, y, w, h]` in Pixeln.
pub type Quad = [f32; 4];

/// Schnitt zweier Rechtecke; `None`, wenn leer.
pub fn intersect(a: Quad, b: Quad) -> Option<Quad> {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = (a[0] + a[2]).min(b[0] + b[2]);
    let y1 = (a[1] + a[3]).min(b[1] + b[3]);
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// Beschneidet ein Ziel-Rechteck auf `clip` und verschiebt/verkleinert das Quell-Rechteck
/// (Textur-Koordinaten in Pixeln) im selben Verhältnis.
pub fn clip_quad(dst: Quad, src: Quad, clip: Quad) -> Option<(Quad, Quad)> {
    let d = intersect(dst, clip)?;
    let sx = src[2] / dst[2];
    let sy = src[3] / dst[3];
    let s = [
        src[0] + (d[0] - dst[0]) * sx,
        src[1] + (d[1] - dst[1]) * sy,
        d[2] * sx,
        d[3] * sy,
    ];
    Some((d, s))
}

/// Einfacher Regal-Packer: Glyphen zeilenweise von links nach rechts, 1 px Abstand.
pub struct ShelfPacker {
    size: u32,
    x: u32,
    y: u32,
    row_h: u32,
}

const PAD: u32 = 1;

impl ShelfPacker {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            x: 0,
            y: 0,
            row_h: 0,
        }
    }

    pub fn reset(&mut self) {
        self.x = 0;
        self.y = 0;
        self.row_h = 0;
    }

    /// Platz für `w`×`h` reservieren; `None`, wenn der Atlas voll ist.
    pub fn alloc(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + PAD > self.size || h + PAD > self.size {
            return None;
        }
        if self.x + w + PAD > self.size {
            self.x = 0;
            self.y += self.row_h;
            self.row_h = 0;
        }
        if self.y + h + PAD > self.size {
            return None;
        }
        let pos = (self.x, self.y);
        self.x += w + PAD;
        self.row_h = self.row_h.max(h + PAD);
        Some(pos)
    }
}

/// sRGB-Wert (0..=255) → linear (0..1), für Surfaces mit sRGB-Format.
pub fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping() {
        assert_eq!(
            intersect([0.0, 0.0, 10.0, 10.0], [5.0, 5.0, 10.0, 10.0]),
            Some([5.0, 5.0, 5.0, 5.0])
        );
        assert_eq!(
            intersect([0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 5.0, 5.0]),
            None
        );
        // Glyph 8×10 bei (2, 3), Atlas-Position (100, 200); Clip schneidet links 2 px und unten 4 px ab
        let (d, s) = clip_quad(
            [2.0, 3.0, 8.0, 10.0],
            [100.0, 200.0, 8.0, 10.0],
            [4.0, 0.0, 100.0, 9.0],
        )
        .unwrap();
        assert_eq!(d, [4.0, 3.0, 6.0, 6.0]);
        assert_eq!(s, [102.0, 200.0, 6.0, 6.0]);
        assert!(clip_quad([0.0, 0.0, 4.0, 4.0], [0.0; 4], [10.0, 10.0, 5.0, 5.0]).is_none());
    }

    #[test]
    fn packing() {
        let mut p = ShelfPacker::new(20);
        assert_eq!(p.alloc(8, 5), Some((0, 0)));
        assert_eq!(p.alloc(8, 7), Some((9, 0)));
        // passt nicht mehr in die Zeile → neue Zeile unter der höchsten Glyphe (7 + 1)
        assert_eq!(p.alloc(8, 5), Some((0, 8)));
        assert_eq!(p.alloc(30, 5), None);
        assert_eq!(p.alloc(8, 12), None); // y 14 + 12 > 20
        p.reset();
        assert_eq!(p.alloc(8, 5), Some((0, 0)));
    }

    #[test]
    fn srgb() {
        assert_eq!(srgb_to_linear(0), 0.0);
        assert!((srgb_to_linear(255) - 1.0).abs() < 1e-6);
        assert!((srgb_to_linear(128) - 0.2158).abs() < 1e-3);
    }
}
