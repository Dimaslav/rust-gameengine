use std::collections::HashMap;

/// Размеры полос атласа (высота в пикселях).
pub const SIZE_BANDS: &[f32] = &[10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 32.0, 48.0];

const ATLAS_W: u32 = 2048;
const ATLAS_H: u32 = 2048;
const GLYPH_PADDING: u32 = 1;
const WHITE_SIZE: u32 = 1;

// ============================================================
// Legacy bitmap constants (для fallback)
// ============================================================

pub const GLYPH_W: u32 = 5;
pub const GLYPH_H: u32 = 7;
pub const GLYPH_CELL_W: u32 = 6;
pub const GLYPH_CELL_H: u32 = 8;
pub const ATLAS_COLS: u32 = 16;
pub const ATLAS_ROWS: u32 = 6;
pub const BITMAP_ATLAS_W: u32 = 128;
pub const BITMAP_ATLAS_H: u32 = 64;
pub const WHITE_BLOCK_X: u32 = 120;
pub const WHITE_BLOCK_Y: u32 = 56;
pub const WHITE_BLOCK_SIZE: u32 = 4;

// ============================================================
// Публичные типы
// ============================================================

#[derive(Clone, Copy, Debug)]
pub struct GlyphMetrics {
    /// UV в атласе: [u0, v0, u1, v1].
    pub uv: [f32; 4],
    /// Размер quad'а в пикселях экрана.
    pub draw_w: f32,
    pub draw_h: f32,
    /// Смещение от левого-верхнего угла текущей строки текста.
    pub offset_x: f32,
    pub offset_y: f32,
    /// Продвижение пера (px).
    pub advance: f32,
}

struct Band {
    glyphs: HashMap<char, GlyphMetrics>,
    /// Расстояние от верхнего края строки до baseline.
    ascent: f32,
    /// Полная высота строки (ascent + descent + gap).
    line_height: f32,
}

pub struct Font {
    pub atlas: Vec<u8>,
    pub atlas_w: u32,
    pub atlas_h: u32,
    /// UV белого 1×1-блока (для solid-квадов).
    pub white_uv: [f32; 4],
    bands: Vec<Band>,
    sizes: Vec<f32>,
    backend: Backend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Ttf,
    Bitmap,
}

// ============================================================
// Загрузка / инициализация
// ============================================================

impl Font {
    /// Основная точка входа. Пытается загрузить TTF, иначе — bitmap.
    pub fn load_default() -> Self {
        match Self::try_load_ttf() {
            Ok(f) => {
                log::info!(
                    "Font: TTF backend, atlas {}×{}, {} bands",
                    f.atlas_w, f.atlas_h, f.bands.len(),
                );
                f
            }
            Err(e) => {
                log::warn!("Font: TTF load failed ({}), using bitmap fallback", e);
                Self::builtin_bitmap()
            }
        }
    }

    fn try_load_ttf() -> Result<Self, String> {
        let bytes = Self::find_ttf_bytes()
            .ok_or_else(|| "no TTF available".to_string())?;
        let font = fontdue::Font::from_bytes(&bytes[..], fontdue::FontSettings::default())
            .map_err(|e| format!("fontdue parse: {e}"))?;
        Self::bake_ttf(&font)
    }

    fn find_ttf_bytes() -> Option<Vec<u8>> {
        // 1. Bundled assets (если пользователь положил)
        for p in [
            "assets/fonts/DejaVuSans.ttf",
            "assets/fonts/Inter-Regular.ttf",
            "assets/fonts/Inter.ttf",
            "assets/fonts/Roboto-Regular.ttf",
            "assets/fonts/NotoSans-Regular.ttf",
        ] {
            if let Ok(b) = std::fs::read(p) {
                log::info!("Font: loaded bundled '{}'", p);
                return Some(b);
            }
        }

        // 2. Windows system fonts (все с полной кириллицей)
        #[cfg(windows)]
        {
            for p in [
                "C:\\Windows\\Fonts\\segoeui.ttf",
                "C:\\Windows\\Fonts\\arial.ttf",
                "C:\\Windows\\Fonts\\tahoma.ttf",
            ] {
                if let Ok(b) = std::fs::read(p) {
                    log::info!("Font: loaded system font '{}'", p);
                    return Some(b);
                }
            }
        }

        None
    }

    fn bake_ttf(font: &fontdue::Font) -> Result<Self, String> {
        let charset = build_charset();

        let mut atlas = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];
        let inv_w = 1.0 / ATLAS_W as f32;
        let inv_h = 1.0 / ATLAS_H as f32;

        // Резервируем 1×1 белый пиксель в нижнем-правом углу.
        let white_x = ATLAS_W - 4;
        let white_y = ATLAS_H - 4;
        {
            let idx = ((white_y * ATLAS_W + white_x) * 4) as usize;
            atlas[idx] = 255;
            atlas[idx + 1] = 255;
            atlas[idx + 2] = 255;
            atlas[idx + 3] = 255;
        }
        let white_uv = [
            (white_x as f32 + 0.5) * inv_w,
            (white_y as f32 + 0.5) * inv_h,
            (white_x as f32 + 0.5) * inv_w,
            (white_y as f32 + 0.5) * inv_h,
        ];

        let mut cursor_x = 0u32;
        let mut cursor_y = 0u32;
        let mut row_h = 0u32;

        let mut bands: Vec<Band> = Vec::with_capacity(SIZE_BANDS.len());

        for &size in SIZE_BANDS {
            let lm = font.horizontal_line_metrics(size);
            let ascent = lm.map(|m| m.ascent).unwrap_or(size * 0.8);
            let line_height = lm.map(|m| m.new_line_size).unwrap_or(size * 1.2);

            let mut glyphs = HashMap::with_capacity(charset.len());

            // Растеризация, сортировка по высоте (лучший shelf-packing).
            let mut items: Vec<(char, fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(charset.len());
            for &c in &charset {
                let (m, b) = font.rasterize(c, size);
                items.push((c, m, b));
            }
            items.sort_by(|a, b| {
                b.1.height.cmp(&a.1.height).then(b.1.width.cmp(&a.1.width))
            });

            for (c, m, bitmap) in items {
                if m.width == 0 || m.height == 0 {
                    // Пробельные глифы — только advance, без quad'а.
                    glyphs.insert(c, GlyphMetrics {
                        uv: [0.0; 4],
                        draw_w: 0.0,
                        draw_h: 0.0,
                        offset_x: 0.0,
                        offset_y: 0.0,
                        advance: m.advance_width,
                    });
                    continue;
                }

                let w = m.width as u32;
                let h = m.height as u32;
                let total_w = w + GLYPH_PADDING * 2;
                let total_h = h + GLYPH_PADDING * 2;

                // Не залезаем в зону белого пикселя в углу.
                let safe_x_limit = ATLAS_W.saturating_sub(WHITE_SIZE + 4);
                let safe_y_limit = ATLAS_H.saturating_sub(WHITE_SIZE + 4);

                if cursor_x + total_w > safe_x_limit {
                    cursor_x = 0;
                    cursor_y += row_h;
                    row_h = 0;
                }
                if cursor_y + total_h > safe_y_limit {
                    log::warn!(
                        "Font: atlas overflow at size {}, char '{}' (0x{:04X})",
                        size, c, c as u32,
                    );
                    continue;
                }

                let dst_x = cursor_x + GLYPH_PADDING;
                let dst_y = cursor_y + GLYPH_PADDING;

                for (i, &alpha) in bitmap.iter().enumerate() {
                    if alpha == 0 { continue; }
                    let gx = (i as u32) % w;
                    let gy = (i as u32) / w;
                    let px = dst_x + gx;
                    let py = dst_y + gy;
                    let idx = ((py * ATLAS_W + px) * 4) as usize;
                    atlas[idx] = 255;
                    atlas[idx + 1] = 255;
                    atlas[idx + 2] = 255;
                    atlas[idx + 3] = alpha;
                }

                let uv = [
                    dst_x as f32 * inv_w,
                    dst_y as f32 * inv_h,
                    (dst_x + w) as f32 * inv_w,
                    (dst_y + h) as f32 * inv_h,
                ];

                glyphs.insert(c, GlyphMetrics {
                    uv,
                    draw_w: w as f32,
                    draw_h: h as f32,
                    offset_x: m.xmin as f32,
                    offset_y: ascent - (m.ymin as f32 + h as f32),
                    advance: m.advance_width,
                });

                cursor_x += total_w;
                row_h = row_h.max(total_h);
            }

            bands.push(Band { glyphs, ascent, line_height });

            // Каждая полоса — с новой строки, для предсказуемости раскладки.
            cursor_x = 0;
            cursor_y += row_h;
            row_h = 0;
        }

        Ok(Self {
            atlas,
            atlas_w: ATLAS_W,
            atlas_h: ATLAS_H,
            white_uv,
            bands,
            sizes: SIZE_BANDS.to_vec(),
            backend: Backend::Ttf,
        })
    }

    fn builtin_bitmap() -> Self {
        let mut atlas = vec![0u8; (BITMAP_ATLAS_W * BITMAP_ATLAS_H * 4) as usize];
        for px in atlas.chunks_exact_mut(4) {
            px[0] = 255;
            px[1] = 255;
            px[2] = 255;
            px[3] = 0;
        }

        let inv_w = 1.0 / BITMAP_ATLAS_W as f32;
        let inv_h = 1.0 / BITMAP_ATLAS_H as f32;
        let mut glyphs = HashMap::with_capacity(FONT_GLYPHS.len());

        for (i, glyph) in FONT_GLYPHS.iter().enumerate() {
            let ch = (32 + i) as u8 as char;
            let col = (i as u32) % ATLAS_COLS;
            let row = (i as u32) / ATLAS_COLS;
            let x0 = col * GLYPH_CELL_W;
            let y0 = row * GLYPH_CELL_H;

            for gx in 0..GLYPH_W {
                let bits = glyph[gx as usize];
                for gy in 0..GLYPH_H {
                    if (bits >> gy) & 1 != 0 {
                        let px = x0 + gx;
                        let py = y0 + gy;
                        let idx = ((py * BITMAP_ATLAS_W + px) * 4) as usize;
                        atlas[idx + 3] = 255;
                    }
                }
            }

            let uv = [
                x0 as f32 * inv_w,
                y0 as f32 * inv_h,
                (x0 + GLYPH_W) as f32 * inv_w,
                (y0 + GLYPH_H) as f32 * inv_h,
            ];

            glyphs.insert(ch, GlyphMetrics {
                uv,
                draw_w: GLYPH_W as f32,
                draw_h: GLYPH_H as f32,
                offset_x: 0.0,
                offset_y: 0.0,
                advance: GLYPH_W as f32,
            });
        }

        // Белый блок 4×4.
        for y in WHITE_BLOCK_Y..WHITE_BLOCK_Y + WHITE_BLOCK_SIZE {
            for x in WHITE_BLOCK_X..WHITE_BLOCK_X + WHITE_BLOCK_SIZE {
                let idx = ((y * BITMAP_ATLAS_W + x) * 4) as usize;
                atlas[idx + 3] = 255;
            }
        }
        let white_x = WHITE_BLOCK_X;
        let white_y = WHITE_BLOCK_Y;
        let white_uv = [
            (white_x as f32 + 0.5) * inv_w,
            (white_y as f32 + 0.5) * inv_h,
            (white_x as f32 + 0.5) * inv_w,
            (white_y as f32 + 0.5) * inv_h,
        ];

        Self {
            atlas,
            atlas_w: BITMAP_ATLAS_W,
            atlas_h: BITMAP_ATLAS_H,
            white_uv,
            bands: vec![Band {
                glyphs,
                ascent: GLYPH_H as f32,
                line_height: GLYPH_CELL_H as f32,
            }],
            sizes: vec![GLYPH_H as f32],
            backend: Backend::Bitmap,
        }
    }

    // ============================================================
    // Запросы
    // ============================================================

    pub fn glyph(&self, c: char, size_px: f32) -> Option<GlyphMetrics> {
        let idx = self.band_for(size_px);
        let band = self.bands.get(idx)?;
        let g = band.glyphs.get(&c)?;

        if self.backend == Backend::Bitmap {
            // Единый native-размер (7), масштабируем.
            let scale = size_px / self.sizes[idx];
            return Some(GlyphMetrics {
                uv: g.uv,
                draw_w: g.draw_w * scale,
                draw_h: size_px,
                offset_x: 0.0,
                offset_y: 0.0,
                advance: g.advance * scale,
            });
        }

        let band_size = self.sizes[idx];
        let scale = size_px / band_size;
        Some(GlyphMetrics {
            uv: g.uv,
            draw_w: g.draw_w * scale,
            draw_h: g.draw_h * scale,
            offset_x: g.offset_x * scale,
            offset_y: g.offset_y * scale,
            advance: g.advance * scale,
        })
    }

    fn band_for(&self, size_px: f32) -> usize {
        let mut best = 0usize;
        let mut best_diff = f32::INFINITY;
        for (i, &s) in self.sizes.iter().enumerate() {
            let d = (s - size_px).abs();
            if d < best_diff {
                best_diff = d;
                best = i;
            }
        }
        best
    }

    pub fn line_height(&self, size_px: f32) -> f32 {
        let idx = self.band_for(size_px);
        let band = &self.bands[idx];
        let scale = size_px / self.sizes[idx];
        band.line_height * scale
    }

    /// Возвращает (ширина, высота) многострочного текста.
    pub fn measure(&self, text: &str, size_px: f32) -> (f32, f32) {
        let idx = self.band_for(size_px);
        let band = &self.bands[idx];
        let scale = size_px / self.sizes[idx];
        let line_h = band.line_height * scale;

        let mut max_w = 0.0f32;
        let mut cur_w = 0.0f32;
        let mut lines = 1u32;

        for c in text.chars() {
            if c == '\n' {
                max_w = max_w.max(cur_w);
                cur_w = 0.0;
                lines += 1;
                continue;
            }
            if let Some(g) = band.glyphs.get(&c) {
                cur_w += g.advance * scale;
            } else {
                cur_w += size_px * 0.4;
            }
        }
        max_w = max_w.max(cur_w);
        (max_w, line_h * lines as f32)
    }
}

// ============================================================
// Charset
// ============================================================

fn build_charset() -> Vec<char> {
    let mut set = std::collections::BTreeSet::new();
    // ASCII printable
    for c in 32u8..=126 {
        set.insert(c as char);
    }
    // Latin-1 supplement
    for c in '\u{00A0}'..='\u{00FF}' {
        set.insert(c);
    }
    // Cyrillic (основной + дополнение)
    for c in '\u{0400}'..='\u{04FF}' {
        set.insert(c);
    }
    // UI-символы
    for c in "─│┌┐└┘├┤┬┴┼←↑→↓↔⇐⇒⇑⇓★☆♥♦♣♠█▓▒░✓✗•·…—–«»„".chars() {
        set.insert(c);
    }
    set.insert('\u{201C}');
    set.insert('\u{201D}');
    set.insert('\u{2018}');
    set.insert('\u{2019}');
    set.into_iter().collect()
}

// ============================================================
// Bitmap fallback (старый 5×7 ASCII)
// ============================================================

#[rustfmt::skip]
pub const FONT_GLYPHS: [[u8; 5]; 96] = [
    [0x00, 0x00, 0x00, 0x00, 0x00],
    [0x00, 0x00, 0x5F, 0x00, 0x00],
    [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7F, 0x14, 0x7F, 0x14],
    [0x24, 0x2A, 0x7F, 0x2A, 0x12],
    [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50],
    [0x00, 0x05, 0x03, 0x00, 0x00],
    [0x00, 0x1C, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1C, 0x00],
    [0x14, 0x08, 0x3E, 0x08, 0x14],
    [0x08, 0x08, 0x3E, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00],
    [0x08, 0x08, 0x08, 0x08, 0x08],
    [0x00, 0x60, 0x60, 0x00, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02],
    [0x3E, 0x51, 0x49, 0x45, 0x3E],
    [0x00, 0x42, 0x7F, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46],
    [0x21, 0x41, 0x45, 0x4B, 0x31],
    [0x18, 0x14, 0x12, 0x7F, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39],
    [0x3C, 0x4A, 0x49, 0x49, 0x30],
    [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36],
    [0x06, 0x49, 0x49, 0x29, 0x1E],
    [0x00, 0x36, 0x36, 0x00, 0x00],
    [0x00, 0x56, 0x36, 0x00, 0x00],
    [0x08, 0x14, 0x22, 0x41, 0x00],
    [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08],
    [0x02, 0x01, 0x51, 0x09, 0x06],
    [0x32, 0x49, 0x79, 0x41, 0x3E],
    [0x7E, 0x11, 0x11, 0x11, 0x7E],
    [0x7F, 0x49, 0x49, 0x49, 0x36],
    [0x3E, 0x41, 0x41, 0x41, 0x22],
    [0x7F, 0x41, 0x41, 0x22, 0x1C],
    [0x7F, 0x49, 0x49, 0x49, 0x41],
    [0x7F, 0x09, 0x09, 0x09, 0x01],
    [0x3E, 0x41, 0x49, 0x49, 0x7A],
    [0x7F, 0x08, 0x08, 0x08, 0x7F],
    [0x00, 0x41, 0x7F, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3F, 0x01],
    [0x7F, 0x08, 0x14, 0x22, 0x41],
    [0x7F, 0x40, 0x40, 0x40, 0x40],
    [0x7F, 0x02, 0x0C, 0x02, 0x7F],
    [0x7F, 0x04, 0x08, 0x10, 0x7F],
    [0x3E, 0x41, 0x41, 0x41, 0x3E],
    [0x7F, 0x09, 0x09, 0x09, 0x06],
    [0x3E, 0x41, 0x51, 0x21, 0x5E],
    [0x7F, 0x09, 0x19, 0x29, 0x46],
    [0x46, 0x49, 0x49, 0x49, 0x31],
    [0x01, 0x01, 0x7F, 0x01, 0x01],
    [0x3F, 0x40, 0x40, 0x40, 0x3F],
    [0x1F, 0x20, 0x40, 0x20, 0x1F],
    [0x3F, 0x40, 0x38, 0x40, 0x3F],
    [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x07, 0x08, 0x70, 0x08, 0x07],
    [0x61, 0x51, 0x49, 0x45, 0x43],
    [0x00, 0x7F, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20],
    [0x00, 0x41, 0x41, 0x7F, 0x00],
    [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40],
    [0x00, 0x01, 0x02, 0x04, 0x00],
    [0x20, 0x54, 0x54, 0x54, 0x78],
    [0x7F, 0x48, 0x44, 0x44, 0x38],
    [0x38, 0x44, 0x44, 0x44, 0x20],
    [0x38, 0x44, 0x44, 0x48, 0x7F],
    [0x38, 0x54, 0x54, 0x54, 0x18],
    [0x08, 0x7E, 0x09, 0x01, 0x02],
    [0x0C, 0x52, 0x52, 0x52, 0x3E],
    [0x7F, 0x08, 0x04, 0x04, 0x78],
    [0x00, 0x44, 0x7D, 0x40, 0x00],
    [0x20, 0x40, 0x44, 0x3D, 0x00],
    [0x7F, 0x10, 0x28, 0x44, 0x00],
    [0x00, 0x41, 0x7F, 0x40, 0x00],
    [0x7C, 0x04, 0x18, 0x04, 0x78],
    [0x7C, 0x08, 0x04, 0x04, 0x78],
    [0x38, 0x44, 0x44, 0x44, 0x38],
    [0x7C, 0x14, 0x14, 0x14, 0x08],
    [0x08, 0x14, 0x14, 0x18, 0x7C],
    [0x7C, 0x08, 0x04, 0x04, 0x08],
    [0x48, 0x54, 0x54, 0x54, 0x20],
    [0x04, 0x3F, 0x44, 0x40, 0x20],
    [0x3C, 0x40, 0x40, 0x20, 0x7C],
    [0x1C, 0x20, 0x40, 0x20, 0x1C],
    [0x3C, 0x40, 0x30, 0x40, 0x3C],
    [0x44, 0x28, 0x10, 0x28, 0x44],
    [0x0C, 0x50, 0x50, 0x50, 0x3C],
    [0x44, 0x64, 0x54, 0x4C, 0x44],
    [0x00, 0x08, 0x36, 0x41, 0x00],
    [0x00, 0x00, 0x7F, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00],
    [0x08, 0x04, 0x08, 0x10, 0x08],
    [0x00, 0x00, 0x00, 0x00, 0x00],
];