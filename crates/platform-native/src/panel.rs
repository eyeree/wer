//! Native bitmap renderer for the shared information-panel document.
//!
//! `viewer-host` owns sampling, section order, labels, values, visibility,
//! and severity. This module owns only the final `font8x8` pixels used by the
//! native map strip and headless screenshots (native/web alignment M6).

use font8x8::legacy::BASIC_LEGACY;
use viewer_host::panel::{PanelColumn, PanelField, PanelSection, PanelSpan, Severity};

/// Panel width in pixels: three legacy stat-column strides plus margins.
pub const PANEL_WIDTH: usize = 3 * COL_CHARS * 8 * SCALE + 2 * MARGIN;

const COL_CHARS: usize = 17;
const SCALE: usize = 2;
const PANEL_SCALE: usize = 1;
const PANEL_LINE_HEIGHT: usize = 10;
const COLUMN_WIDTH: usize = COL_CHARS * 8 * SCALE;
const COLUMN_CHARS: usize = COLUMN_WIDTH / (8 * PANEL_SCALE);
const MARGIN: usize = 12;

const BG: [u8; 3] = [16, 18, 24];
const RULE: [u8; 3] = [50, 54, 68];
const TITLE: [u8; 3] = [255, 255, 255];
const HEADER: [u8; 3] = [120, 170, 255];
const LABEL: [u8; 3] = [130, 135, 150];
const VALUE: [u8; 3] = [225, 228, 235];
const WARNING: [u8; 3] = [255, 200, 90];
const ERROR: [u8; 3] = [255, 90, 90];

/// Split text into lossless display lines no wider than `max_chars`.
///
/// Whitespace is kept at the end of the preceding line so joining the result
/// reproduces the source exactly. When a token is wider than the column, the
/// token is hard-split instead of relying on the rasterizer to clip it.
fn wrap_lines(text: &str, max_chars: usize) -> Vec<String> {
    debug_assert!(max_chars > 0);
    if text.is_empty() {
        return vec![String::new()];
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let hard_end = (start + max_chars).min(chars.len());
        let end = if hard_end == chars.len() {
            hard_end
        } else {
            chars[start..hard_end]
                .iter()
                .rposition(|ch| ch.is_whitespace())
                .map_or(hard_end, |offset| start + offset + 1)
        };
        lines.push(chars[start..end].iter().copied().collect());
        start = end;
    }
    lines
}

/// Rasterizes shared panel sections beside a square map.
#[derive(Debug)]
pub struct Hud {
    map_side: usize,
    pixels: Vec<u8>,
    panel_scratch: Vec<u8>,
    panel_scratch_revision: Option<u64>,
}

impl Hud {
    #[must_use]
    pub fn new(map_side: usize) -> Self {
        Self {
            map_side,
            pixels: vec![0; (map_side + PANEL_WIDTH) * map_side * 4],
            panel_scratch: Vec::new(),
            panel_scratch_revision: None,
        }
    }

    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        ((self.map_side + PANEL_WIDTH) as u32, self.map_side as u32)
    }

    /// Draw the shared sections into the standalone strip used by the GPU-map
    /// path. No semantic value is interpreted here.
    pub fn panel_image(&mut self, sections: &[PanelSection]) -> (&[u8], u32, u32) {
        self.render_panel_scratch(sections);
        // This entry point has no semantic key. A later keyed request must
        // conservatively rasterize once rather than trust unknown contents.
        self.panel_scratch_revision = None;
        self.panel_scratch_parts()
    }

    /// Return the standalone panel strip for one stable shared-document
    /// revision, rasterizing only when that semantic revision changes.
    pub fn panel_image_for(
        &mut self,
        revision: u64,
        sections: &[PanelSection],
    ) -> (&[u8], u32, u32, bool) {
        let changed = self.panel_scratch_revision != Some(revision);
        if changed {
            self.render_panel_scratch(sections);
            self.panel_scratch_revision = Some(revision);
        }
        let (pixels, width, height) = self.panel_scratch_parts();
        (pixels, width, height, changed)
    }

    fn render_panel_scratch(&mut self, sections: &[PanelSection]) {
        self.clear_panel();
        self.draw_panel(sections);
        let width = self.map_side + PANEL_WIDTH;
        self.panel_scratch
            .resize(PANEL_WIDTH * self.map_side * 4, 0);
        for row in 0..self.map_side {
            let src = (row * width + self.map_side) * 4;
            let dst = row * PANEL_WIDTH * 4;
            self.panel_scratch[dst..dst + PANEL_WIDTH * 4]
                .copy_from_slice(&self.pixels[src..src + PANEL_WIDTH * 4]);
        }
    }

    fn panel_scratch_parts(&self) -> (&[u8], u32, u32) {
        (
            &self.panel_scratch,
            PANEL_WIDTH as u32,
            self.map_side as u32,
        )
    }

    #[must_use]
    pub fn panel_pixels(&self) -> &[u8] {
        &self.panel_scratch
    }

    /// Blit the map and rasterize the exact same shared sections used live.
    pub fn compose(&mut self, map_rgba: &[u8], sections: &[PanelSection]) -> &[u8] {
        let width = self.map_side + PANEL_WIDTH;
        for row in 0..self.map_side {
            let src = row * self.map_side * 4;
            let dst = row * width * 4;
            self.pixels[dst..dst + self.map_side * 4]
                .copy_from_slice(&map_rgba[src..src + self.map_side * 4]);
        }
        self.clear_panel();
        self.draw_panel(sections);
        &self.pixels
    }

    fn clear_panel(&mut self) {
        let width = self.map_side + PANEL_WIDTH;
        for row in 0..self.map_side {
            let start = (row * width + self.map_side) * 4;
            for px in self.pixels[start..start + PANEL_WIDTH * 4].chunks_exact_mut(4) {
                px.copy_from_slice(&[BG[0], BG[1], BG[2], 255]);
            }
        }
    }

    fn draw_panel(&mut self, sections: &[PanelSection]) {
        let left = self.map_side + MARGIN;
        self.text(
            left,
            MARGIN,
            "INFINITE WORLD",
            TITLE,
            SCALE,
            self.map_side + PANEL_WIDTH - MARGIN,
        );
        let top = MARGIN + 8 * SCALE + PANEL_LINE_HEIGHT;
        for column in [
            PanelColumn::Explorer,
            PanelColumn::World,
            PanelColumn::System,
        ] {
            let x = left + column.index() * COLUMN_WIDTH;
            let mut cursor = PanelCursor {
                x,
                max_x: (x + COLUMN_WIDTH).min(self.map_side + PANEL_WIDTH - MARGIN),
                y: top,
            };
            for section in sections.iter().filter(|section| section.column == column) {
                if !section.fields.iter().any(|field| field.visible) {
                    continue;
                }
                cursor.line(self, &section.title.to_ascii_uppercase(), HEADER);
                for field in section.fields.iter().filter(|field| field.visible) {
                    cursor.field(self, field);
                }
                cursor.rule(self);
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, text: &str, color: [u8; 3], scale: usize, max_x: usize) {
        let width = self.map_side + PANEL_WIDTH;
        let height = self.map_side;
        let mut pen_x = x;
        for ch in text.chars() {
            let glyph = BASIC_LEGACY[if ch.is_ascii() {
                ch as usize
            } else {
                b'?' as usize
            }];
            for (gy, row) in glyph.iter().enumerate() {
                for gx in 0..8usize {
                    if row >> gx & 1 == 0 {
                        continue;
                    }
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = pen_x + gx * scale + sx;
                            let py = y + gy * scale + sy;
                            if px >= width || px >= max_x || py >= height {
                                continue;
                            }
                            let offset = (py * width + px) * 4;
                            self.pixels[offset..offset + 3].copy_from_slice(&color);
                        }
                    }
                }
            }
            pen_x += 8 * scale;
        }
    }

    fn hline(&mut self, x: usize, max_x: usize, y: usize) {
        let width = self.map_side + PANEL_WIDTH;
        if y >= self.map_side {
            return;
        }
        for px in x..max_x.min(width - MARGIN) {
            let offset = (y * width + px) * 4;
            self.pixels[offset..offset + 3].copy_from_slice(&RULE);
        }
    }
}

struct PanelCursor {
    x: usize,
    max_x: usize,
    y: usize,
}

impl PanelCursor {
    fn line(&mut self, hud: &mut Hud, text: &str, color: [u8; 3]) {
        hud.text(self.x, self.y, text, color, PANEL_SCALE, self.max_x);
        self.y += PANEL_LINE_HEIGHT;
    }

    fn field(&mut self, hud: &mut Hud, field: &PanelField) {
        let color = match field.severity {
            Severity::Info => VALUE,
            Severity::Warning => WARNING,
            Severity::Error => ERROR,
        };
        let label_chars = field.label.chars().count();
        let combined_chars = label_chars + 1 + field.value.chars().count();
        let separate = field.span == PanelSpan::Wide || combined_chars > COLUMN_CHARS;
        hud.text(self.x, self.y, field.label, LABEL, PANEL_SCALE, self.max_x);
        if separate {
            self.y += PANEL_LINE_HEIGHT;
            let value_x = self.x + 8 * PANEL_SCALE;
            let value_chars = (self.max_x - value_x) / (8 * PANEL_SCALE);
            for line in wrap_lines(&field.value, value_chars) {
                hud.text(value_x, self.y, &line, color, PANEL_SCALE, self.max_x);
                self.y += PANEL_LINE_HEIGHT;
            }
        } else {
            let value_x = self.x + (label_chars + 1) * 8 * PANEL_SCALE;
            hud.text(
                value_x,
                self.y,
                &field.value,
                color,
                PANEL_SCALE,
                self.max_x,
            );
            self.y += PANEL_LINE_HEIGHT;
        }
    }

    fn rule(&mut self, hud: &mut Hud) {
        self.y += PANEL_LINE_HEIGHT / 3;
        hud.hline(self.x, self.max_x, self.y);
        self.y += PANEL_LINE_HEIGHT * 2 / 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_host::panel::{PanelColumn, PanelFieldId, PanelSpan};

    fn sections(value: &str, severity: Severity) -> Vec<PanelSection> {
        vec![PanelSection {
            id: "fixture",
            title: "Fixture",
            column: PanelColumn::Explorer,
            span: PanelSpan::Single,
            fields: vec![PanelField {
                id: PanelFieldId::new("fixture.value"),
                label: "Value",
                value: String::from(value),
                severity,
                span: PanelSpan::Single,
                visible: true,
            }],
        }]
    }

    #[test]
    fn shared_field_value_and_severity_drive_native_pixels() {
        let side = 256usize;
        let map = vec![0u8; side * side * 4];
        let mut hud = Hud::new(side);
        let first = hud
            .compose(&map, &sections("alpha", Severity::Info))
            .to_vec();
        let second = hud
            .compose(&map, &sections("beta", Severity::Error))
            .to_vec();
        assert_ne!(first, second);
        assert!(second.chunks_exact(4).any(|pixel| pixel[..3] == ERROR));
    }

    #[test]
    fn stable_document_revision_reuses_the_panel_raster() {
        let mut hud = Hud::new(256);
        let sections = sections("alpha", Severity::Info);
        let (first, _, _, rasterized) = hud.panel_image_for(7, &sections);
        assert!(rasterized);
        let first = first.to_vec();

        let (second, _, _, rasterized) = hud.panel_image_for(7, &sections);
        assert!(!rasterized);
        assert_eq!(second, first);

        // An unkeyed caller invalidates the semantic cache conservatively.
        hud.panel_image(&sections);
        assert!(hud.panel_image_for(7, &sections).3);
        assert!(hud.panel_image_for(8, &sections).3);
    }

    #[test]
    fn semantic_columns_rasterize_into_three_distinct_native_columns() {
        let side = 256usize;
        let sections = [
            (PanelColumn::Explorer, Severity::Info, VALUE),
            (PanelColumn::World, Severity::Warning, WARNING),
            (PanelColumn::System, Severity::Error, ERROR),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (column, severity, _))| PanelSection {
            id: ["explorer", "world", "system"][index],
            title: "Fixture",
            column,
            span: PanelSpan::Single,
            fields: vec![PanelField {
                id: PanelFieldId::new(
                    ["fixture.explorer", "fixture.world", "fixture.system"][index],
                ),
                label: "Value",
                value: String::from("visible"),
                severity,
                span: PanelSpan::Single,
                visible: true,
            }],
        })
        .collect::<Vec<_>>();
        let mut hud = Hud::new(side);
        let pixels = hud.panel_image(&sections).0.to_vec();
        for (column, (_, _, expected)) in [
            (0, (PanelColumn::Explorer, Severity::Info, VALUE)),
            (1, (PanelColumn::World, Severity::Warning, WARNING)),
            (2, (PanelColumn::System, Severity::Error, ERROR)),
        ] {
            let start = MARGIN + column * COLUMN_WIDTH;
            let end = (start + COLUMN_WIDTH).min(PANEL_WIDTH);
            assert!((0..side).any(|y| {
                (start..end).any(|x| {
                    let offset = (y * PANEL_WIDTH + x) * 4;
                    pixels[offset..offset + 3] == expected
                })
            }));
        }
    }

    #[test]
    fn wrap_lines_preserves_all_text_and_hard_splits_unbroken_tokens() {
        let value = "alpha beta abcdefghijklmnopqrstuvwxyz0123456789 omega";
        let lines = wrap_lines(value, 12);

        assert_eq!(lines.concat(), value);
        assert!(lines.iter().all(|line| line.chars().count() <= 12));
        assert!(lines.iter().any(|line| line == "abcdefghijkl"));
        assert!(lines.iter().any(|line| line == "mnopqrstuvwx"));
    }

    #[test]
    fn long_field_value_rasterizes_every_line_without_cross_column_spill() {
        let side = 256usize;
        let value = "A".repeat(COLUMN_CHARS * 2 + 5);
        let expected_lines = wrap_lines(&value, COLUMN_CHARS - 1);
        assert_eq!(expected_lines.len(), 3);
        assert_eq!(expected_lines.concat(), value);

        let mut hud = Hud::new(side);
        let pixels = hud
            .panel_image(&sections(&value, Severity::Error))
            .0
            .to_vec();
        let value_top = MARGIN + 8 * SCALE + PANEL_LINE_HEIGHT * 3;
        for line in 0..expected_lines.len() {
            let top = value_top + line * PANEL_LINE_HEIGHT;
            assert!((top..top + 8).any(|y| {
                (MARGIN..MARGIN + COLUMN_WIDTH).any(|x| {
                    let offset = (y * PANEL_WIDTH + x) * 4;
                    pixels[offset..offset + 3] == ERROR
                })
            }));
        }
        assert!((0..side).all(|y| {
            (MARGIN + COLUMN_WIDTH..PANEL_WIDTH).all(|x| {
                let offset = (y * PANEL_WIDTH + x) * 4;
                pixels[offset..offset + 3] != ERROR
            })
        }));
    }
}
