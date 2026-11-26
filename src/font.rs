use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context};
use fontdue::{Font, FontSettings, Metrics};

/// Ein gerasterter Glyph: Metriken + 8-Bit-Abdeckung (width * height).
pub struct Glyph {
    pub metrics: Metrics,
    pub coverage: Vec<u8>,
}

/// Monospace-Schrift mit Glyph-Cache. Zellgröße wird aus der Schrift abgeleitet.
pub struct FontAtlas {
    font: Font,
    px: f32,
    pub cell_w: usize,
    pub cell_h: usize,
    pub ascent: f32,
    /// Eindeutig pro geladener Schrift/Größe – der GPU-Atlas erkennt daran, dass er neu laden muss.
    pub generation: u64,
    cache: HashMap<char, Glyph>,
}

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);

impl FontAtlas {
    pub fn load(path: Option<&Path>, size_pt: f32, scale: f64) -> anyhow::Result<Self> {
        let path = match path {
            Some(p) => p.to_path_buf(),
            None => find_monospace()?,
        };
        let bytes = std::fs::read(&path)
            .with_context(|| format!("Schrift {} nicht lesbar", path.display()))?;
        // 1 pt = 4/3 px bei 96 dpi
        let px = (size_pt * 4.0 / 3.0 * scale as f32).round();
        let font = Font::from_bytes(
            bytes,
            FontSettings {
                scale: px,
                ..FontSettings::default()
            },
        )
        .map_err(|e| anyhow!("Schrift {}: {e}", path.display()))?;

        let lm = font
            .horizontal_line_metrics(px)
            .ok_or_else(|| anyhow!("Schrift {} hat keine horizontalen Metriken", path.display()))?;
        let cell_h = lm.new_line_size.ceil().max(1.0) as usize;
        let cell_w = font.metrics('M', px).advance_width.ceil().max(1.0) as usize;
        log::info!(
            "Schrift {} @ {px}px, Zelle {cell_w}x{cell_h}",
            path.display()
        );

        Ok(Self {
            font,
            px,
            cell_w,
            cell_h,
            ascent: lm.ascent,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            cache: HashMap::new(),
        })
    }

    pub fn glyph(&mut self, c: char) -> &Glyph {
        self.cache.entry(c).or_insert_with(|| {
            let (metrics, coverage) = self.font.rasterize(c, self.px);
            Glyph { metrics, coverage }
        })
    }
}

fn find_monospace() -> anyhow::Result<PathBuf> {
    if let Ok(out) = Command::new("fc-match")
        .args(["-f", "%{file}", "monospace:style=Regular"])
        .output()
    {
        let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if p.is_file() {
            return Ok(p);
        }
    }
    const FALLBACKS: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    ];
    FALLBACKS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .ok_or_else(|| {
            anyhow!(
                "Keine Monospace-Schrift gefunden – bitte [font] path in der config.toml setzen"
            )
        })
}
