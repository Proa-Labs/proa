// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Font metric extraction and metric-matched fallback computation.
//!
//! The layout-shift-free part of the pipeline. We parse the vertical metrics
//! (`ascent`, `descent`, `line-gap`, `units-per-em`) and an average character
//! width (`xWidthAvg`) out of the web font, then synthesize `@font-face`
//! descriptor overrides for a locally-available fallback (Arial, Times New
//! Roman, Courier New) so that swapping from the fallback to the web font does
//! not reflow the page.
//!
//! The algorithm matches the one popularized by Katie Hempenius' "automatic
//! font fallback" work and the `fontaine` / `@capsizecss/metrics` projects:
//!
//! ```text
//! sizeAdjust      = (webfont.xWidthAvg / webfont.unitsPerEm)
//!                 / (fallback.xWidthAvg / fallback.unitsPerEm)
//! ascentOverride  = (webfont.ascent      / webfont.unitsPerEm) / sizeAdjust
//! descentOverride = (|webfont.descent|   / webfont.unitsPerEm) / sizeAdjust
//! lineGapOverride = (webfont.lineGap     / webfont.unitsPerEm) / sizeAdjust
//! ```
//!
//! all expressed as CSS percentages.
//!
//! We parse only the tables needed for metrics (`head`, `hhea`, `OS/2`,
//! `maxp`, `hmtx`, `cmap`). These tables are never transformed by WOFF2, so the
//! same parser serves both raw sfnt (`.ttf`/`.otf`) input and the sfnt we
//! reconstruct from WOFF2 (see [`crate::woff2`]).

use byteorder::{BigEndian, ByteOrder};

use crate::error::BuildError;

/// English lowercase-letter frequencies plus the space, used to weight the
/// average advance width. Matches the weighting `@capsizecss/metrics` uses so
/// our computed `xWidthAvg` is on the same basis as the hardcoded fallback
/// constants below. Values need not sum to exactly 1; we normalize by the sum
/// of the weights actually resolved.
const WEIGHTED_CHARS: &[(char, f64)] = &[
    (' ', 0.181_2),
    ('a', 0.081_67),
    ('b', 0.014_92),
    ('c', 0.027_82),
    ('d', 0.042_53),
    ('e', 0.127_02),
    ('f', 0.022_28),
    ('g', 0.020_15),
    ('h', 0.060_94),
    ('i', 0.069_66),
    ('j', 0.001_53),
    ('k', 0.007_72),
    ('l', 0.040_25),
    ('m', 0.024_06),
    ('n', 0.067_49),
    ('o', 0.075_07),
    ('p', 0.019_29),
    ('q', 0.000_95),
    ('r', 0.058_87),
    ('s', 0.063_27),
    ('t', 0.090_56),
    ('u', 0.027_58),
    ('v', 0.009_78),
    ('w', 0.023_60),
    ('x', 0.001_50),
    ('y', 0.019_74),
    ('z', 0.000_74),
];

/// Vertical and width metrics parsed from a font, in font design units except
/// where noted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    pub ascent: i32,
    pub descent: i32,
    pub line_gap: i32,
    pub cap_height: Option<i32>,
    pub x_height: Option<i32>,
    /// Weighted-average advance width of common lowercase letters, in font
    /// units. `None` when a usable `cmap`/`hmtx` pair could not be read.
    pub x_width_avg: Option<f64>,
}

/// Metrics for a locally-installed fallback family. These are fixed, published
/// constants (from `@capsizecss/metrics`), so the build never needs the
/// fallback font file itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallbackMetrics {
    /// The `local(...)` face the browser already has, e.g. `"Arial"`.
    pub local: &'static str,
    pub units_per_em: f64,
    pub x_width_avg: f64,
}

/// Arial: the sans-serif fallback. From `@capsizecss/metrics` (`arial`).
pub const ARIAL: FallbackMetrics = FallbackMetrics {
    local: "Arial",
    units_per_em: 2048.0,
    x_width_avg: 904.0,
};

/// Times New Roman: the serif fallback. From `@capsizecss/metrics`
/// (`times-new-roman`).
pub const TIMES_NEW_ROMAN: FallbackMetrics = FallbackMetrics {
    local: "Times New Roman",
    units_per_em: 2048.0,
    x_width_avg: 821.0,
};

/// Courier New: the monospace fallback. From `@capsizecss/metrics`
/// (`courier-new`).
pub const COURIER_NEW: FallbackMetrics = FallbackMetrics {
    local: "Courier New",
    units_per_em: 2048.0,
    x_width_avg: 1229.0,
};

/// The CSS `@font-face` descriptor overrides for a metric-matched fallback,
/// pre-formatted as percentage strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackOverrides {
    /// The `local(...)` fallback face name. Defaults to the generic's canonical
    /// fallback (e.g. `Arial`) but a caller may override it.
    pub local: String,
    pub ascent_override: String,
    pub descent_override: String,
    pub line_gap_override: String,
    pub size_adjust: String,
}

/// Compute metric-matched fallback overrides for `webfont` rendered against
/// `fallback`.
///
/// When the web font's average width could not be measured, `size-adjust`
/// stays at 100% and the overrides reduce to the raw normalized metrics, which
/// still removes most of the swap shift.
pub fn compute_fallback(webfont: &FontMetrics, fallback: &FallbackMetrics) -> FallbackOverrides {
    let upm = f64::from(webfont.units_per_em);
    let size_adjust = match webfont.x_width_avg {
        Some(x_avg) if x_avg > 0.0 => {
            let webfont_ratio = x_avg / upm;
            let fallback_ratio = fallback.x_width_avg / fallback.units_per_em;
            if fallback_ratio > 0.0 {
                webfont_ratio / fallback_ratio
            } else {
                1.0
            }
        }
        _ => 1.0,
    };

    let ascent = (f64::from(webfont.ascent) / upm) / size_adjust;
    let descent = (f64::from(webfont.descent).abs() / upm) / size_adjust;
    let line_gap = (f64::from(webfont.line_gap) / upm) / size_adjust;

    FallbackOverrides {
        local: fallback.local.to_string(),
        ascent_override: percent(ascent),
        descent_override: percent(descent),
        line_gap_override: percent(line_gap),
        size_adjust: percent(size_adjust),
    }
}

/// Format a ratio as a CSS percentage with two decimals, e.g. `0.9502` ->
/// `"95.02%"`. Trailing-zero-stable so generated output is deterministic.
fn percent(ratio: f64) -> String {
    format!("{:.2}%", ratio * 100.0)
}

// ---------------------------------------------------------------------------
// sfnt parsing
// ---------------------------------------------------------------------------

const TAG_HEAD: u32 = tag(b"head");
const TAG_HHEA: u32 = tag(b"hhea");
const TAG_OS2: u32 = tag(b"OS/2");
const TAG_HMTX: u32 = tag(b"hmtx");
const TAG_CMAP: u32 = tag(b"cmap");

const fn tag(t: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*t)
}

struct Sfnt<'a> {
    data: &'a [u8],
    /// (tag, offset, length) for each table.
    tables: Vec<(u32, usize, usize)>,
}

impl<'a> Sfnt<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, BuildError> {
        // A TrueType Collection ('ttcf') points at one or more sfnt offset
        // tables; use the first font.
        let base = if data.len() >= 16 && &data[0..4] == b"ttcf" {
            read_u32(data, 12)? as usize
        } else {
            0
        };

        let num_tables = read_u16(data, base + 4)? as usize;
        let mut tables = Vec::with_capacity(num_tables);
        let record_start = base + 12;
        for i in 0..num_tables {
            let rec = record_start + i * 16;
            let t = read_u32(data, rec)?;
            let offset = read_u32(data, rec + 8)? as usize;
            let length = read_u32(data, rec + 12)? as usize;
            tables.push((t, offset, length));
        }
        Ok(Self { data, tables })
    }

    fn table(&self, tag: u32) -> Option<&'a [u8]> {
        let (_, offset, length) = *self.tables.iter().find(|(t, ..)| *t == tag)?;
        self.data.get(offset..offset.checked_add(length)?)
    }

    fn require(&self, tag: u32, name: &'static str) -> Result<&'a [u8], BuildError> {
        self.table(tag).ok_or(BuildError::MissingTable { name })
    }
}

/// Parse the metrics we need out of raw sfnt bytes (`.ttf`/`.otf`, or an sfnt
/// reconstructed from WOFF2).
pub fn parse_sfnt_metrics(data: &[u8]) -> Result<FontMetrics, BuildError> {
    let sfnt = Sfnt::parse(data)?;

    let head = sfnt.require(TAG_HEAD, "head")?;
    let units_per_em = read_u16(head, 18)?;
    if units_per_em == 0 {
        return Err(BuildError::InvalidFont("units_per_em is zero"));
    }

    let hhea = sfnt.require(TAG_HHEA, "hhea")?;
    let hhea_ascent = i32::from(read_i16(hhea, 4)?);
    let hhea_descent = i32::from(read_i16(hhea, 6)?);
    let hhea_line_gap = i32::from(read_i16(hhea, 8)?);
    let num_h_metrics = read_u16(hhea, 34)? as usize;

    // Prefer OS/2 typographic metrics when the font asks us to (USE_TYPO_METRICS
    // bit of fsSelection), which is the modern, cross-platform-consistent
    // source. Otherwise fall back to hhea.
    let os2 = sfnt.table(TAG_OS2);
    let (ascent, descent, line_gap, cap_height, x_height) = match os2 {
        Some(os2) if os2.len() >= 78 => {
            let version = read_u16(os2, 0)?;
            let fs_selection = read_u16(os2, 62)?;
            let use_typo = fs_selection & 0x0080 != 0;
            let typo_ascent = i32::from(read_i16(os2, 68)?);
            let typo_descent = i32::from(read_i16(os2, 70)?);
            let typo_line_gap = i32::from(read_i16(os2, 72)?);
            let (x_height, cap_height) = if version >= 2 && os2.len() >= 90 {
                (
                    Some(i32::from(read_i16(os2, 86)?)),
                    Some(i32::from(read_i16(os2, 88)?)),
                )
            } else {
                (None, None)
            };
            if use_typo {
                (
                    typo_ascent,
                    typo_descent,
                    typo_line_gap,
                    cap_height,
                    x_height,
                )
            } else {
                (
                    hhea_ascent,
                    hhea_descent,
                    hhea_line_gap,
                    cap_height,
                    x_height,
                )
            }
        }
        _ => (hhea_ascent, hhea_descent, hhea_line_gap, None, None),
    };

    let x_width_avg = weighted_x_width_avg(&sfnt, num_h_metrics).ok().flatten();

    Ok(FontMetrics {
        units_per_em,
        ascent,
        descent,
        line_gap,
        cap_height,
        x_height,
        x_width_avg,
    })
}

/// Weighted average advance width of the common lowercase letters, in font
/// units. Returns `Ok(None)` when the font has no usable Unicode `cmap` or the
/// `hmtx` table is unreadable, which is not fatal (fallback size-adjust just
/// stays at 100%).
fn weighted_x_width_avg(sfnt: &Sfnt<'_>, num_h_metrics: usize) -> Result<Option<f64>, BuildError> {
    let (Some(cmap), Some(hmtx)) = (sfnt.table(TAG_CMAP), sfnt.table(TAG_HMTX)) else {
        return Ok(None);
    };
    if num_h_metrics == 0 {
        return Ok(None);
    }
    let Some(mapper) = CmapMapper::parse(cmap) else {
        return Ok(None);
    };

    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;
    for &(ch, weight) in WEIGHTED_CHARS {
        let Some(gid) = mapper.glyph(ch as u32) else {
            continue;
        };
        let Some(advance) = advance_width(hmtx, num_h_metrics, gid) else {
            continue;
        };
        weighted_sum += f64::from(advance) * weight;
        weight_total += weight;
    }

    if weight_total <= 0.0 {
        Ok(None)
    } else {
        Ok(Some(weighted_sum / weight_total))
    }
}

/// Advance width of glyph `gid` from `hmtx`. Glyph ids at or beyond
/// `num_h_metrics` share the last explicit advance (monospaced tail).
fn advance_width(hmtx: &[u8], num_h_metrics: usize, gid: u16) -> Option<u16> {
    let idx = (gid as usize).min(num_h_metrics - 1);
    read_u16(hmtx, idx * 4).ok()
}

/// Minimal `cmap` reader supporting the two subtable formats that cover modern
/// Unicode fonts: format 4 (BMP) and format 12 (full range).
enum CmapMapper<'a> {
    Format4 {
        seg_count: usize,
        end_codes: &'a [u8],
        start_codes: &'a [u8],
        id_deltas: &'a [u8],
        id_range_offsets: &'a [u8],
        id_range_offsets_start: usize,
        table: &'a [u8],
    },
    Format12 {
        groups: &'a [u8],
        num_groups: usize,
    },
}

impl<'a> CmapMapper<'a> {
    fn parse(cmap: &'a [u8]) -> Option<Self> {
        let num_tables = read_u16(cmap, 2).ok()? as usize;
        // Prefer a full-repertoire Unicode subtable, then a BMP one.
        let mut best: Option<(u8, usize)> = None; // (priority, subtable offset)
        for i in 0..num_tables {
            let rec = 4 + i * 8;
            let platform = read_u16(cmap, rec).ok()?;
            let encoding = read_u16(cmap, rec + 2).ok()?;
            let offset = read_u32(cmap, rec + 4).ok()? as usize;
            let priority = match (platform, encoding) {
                (3, 10) | (0, 4) | (0, 6) => 3, // full Unicode
                (3, 1) | (0, 3) => 2,           // BMP Unicode
                (0, _) => 1,                    // any Unicode
                _ => 0,
            };
            if priority > 0 && best.is_none_or(|(p, _)| priority > p) {
                best = Some((priority, offset));
            }
        }
        let (_, offset) = best?;
        let subtable = cmap.get(offset..)?;
        let format = read_u16(subtable, 0).ok()?;
        match format {
            4 => Self::parse_format4(subtable),
            12 => Self::parse_format12(subtable),
            _ => None,
        }
    }

    fn parse_format4(table: &'a [u8]) -> Option<Self> {
        let seg_x2 = read_u16(table, 6).ok()? as usize;
        let seg_count = seg_x2 / 2;
        let end_codes_start = 14;
        let start_codes_start = end_codes_start + seg_x2 + 2; // +2 reservedPad
        let id_deltas_start = start_codes_start + seg_x2;
        let id_range_offsets_start = id_deltas_start + seg_x2;
        let end_codes = table.get(end_codes_start..end_codes_start + seg_x2)?;
        let start_codes = table.get(start_codes_start..start_codes_start + seg_x2)?;
        let id_deltas = table.get(id_deltas_start..id_deltas_start + seg_x2)?;
        let id_range_offsets =
            table.get(id_range_offsets_start..id_range_offsets_start + seg_x2)?;
        Some(Self::Format4 {
            seg_count,
            end_codes,
            start_codes,
            id_deltas,
            id_range_offsets,
            id_range_offsets_start,
            table,
        })
    }

    fn parse_format12(table: &'a [u8]) -> Option<Self> {
        let num_groups = read_u32(table, 12).ok()? as usize;
        let groups = table.get(16..16 + num_groups * 12)?;
        Some(Self::Format12 { groups, num_groups })
    }

    fn glyph(&self, cp: u32) -> Option<u16> {
        match self {
            Self::Format4 {
                seg_count,
                end_codes,
                start_codes,
                id_deltas,
                id_range_offsets,
                id_range_offsets_start,
                table,
            } => {
                if cp > 0xFFFF {
                    return None;
                }
                let cp = cp as u16;
                for seg in 0..*seg_count {
                    let end = BigEndian::read_u16(&end_codes[seg * 2..]);
                    if cp > end {
                        continue;
                    }
                    let start = BigEndian::read_u16(&start_codes[seg * 2..]);
                    if cp < start {
                        return None;
                    }
                    let id_delta = BigEndian::read_u16(&id_deltas[seg * 2..]);
                    let id_range_offset = BigEndian::read_u16(&id_range_offsets[seg * 2..]);
                    if id_range_offset == 0 {
                        return Some(cp.wrapping_add(id_delta));
                    }
                    // The spooky pointer arithmetic mandated by the spec.
                    let glyph_index_addr = id_range_offsets_start
                        + seg * 2
                        + id_range_offset as usize
                        + (cp - start) as usize * 2;
                    let gid = read_u16(table, glyph_index_addr).ok()?;
                    if gid == 0 {
                        return Some(0);
                    }
                    return Some(gid.wrapping_add(id_delta));
                }
                None
            }
            Self::Format12 { groups, num_groups } => {
                for g in 0..*num_groups {
                    let base = g * 12;
                    let start = BigEndian::read_u32(&groups[base..]);
                    let end = BigEndian::read_u32(&groups[base + 4..]);
                    if cp >= start && cp <= end {
                        let start_gid = BigEndian::read_u32(&groups[base + 8..]);
                        return u16::try_from(start_gid + (cp - start)).ok();
                    }
                }
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// bounds-checked big-endian reads
// ---------------------------------------------------------------------------

fn read_u16(data: &[u8], at: usize) -> Result<u16, BuildError> {
    data.get(at..at + 2)
        .map(BigEndian::read_u16)
        .ok_or(BuildError::Truncated)
}

fn read_i16(data: &[u8], at: usize) -> Result<i16, BuildError> {
    data.get(at..at + 2)
        .map(BigEndian::read_i16)
        .ok_or(BuildError::Truncated)
}

fn read_u32(data: &[u8], at: usize) -> Result<u32, BuildError> {
    data.get(at..at + 4)
        .map(BigEndian::read_u32)
        .ok_or(BuildError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_adjust_is_ratio_of_normalized_avg_widths() {
        // Web font wider than Arial -> size-adjust > 100%, and vice versa.
        let webfont = FontMetrics {
            units_per_em: 1000,
            ascent: 950,
            descent: -250,
            line_gap: 0,
            cap_height: Some(700),
            x_height: Some(500),
            x_width_avg: Some(500.0), // 0.5 normalized
        };
        // Arial normalized xAvg = 904/2048 = 0.44140625; 0.5 / 0.4414... = 1.1327...
        let fb = compute_fallback(&webfont, &ARIAL);
        assert_eq!(fb.local, "Arial");
        assert_eq!(fb.size_adjust, "113.27%");
        // ascent = (950/1000)/1.13271 = 0.83867 -> 83.87%
        assert_eq!(fb.ascent_override, "83.87%");
        assert_eq!(fb.descent_override, "22.07%");
        assert_eq!(fb.line_gap_override, "0.00%");
    }

    #[test]
    fn missing_avg_width_leaves_size_adjust_at_100() {
        let webfont = FontMetrics {
            units_per_em: 1000,
            ascent: 800,
            descent: -200,
            line_gap: 100,
            cap_height: None,
            x_height: None,
            x_width_avg: None,
        };
        let fb = compute_fallback(&webfont, &TIMES_NEW_ROMAN);
        assert_eq!(fb.size_adjust, "100.00%");
        assert_eq!(fb.ascent_override, "80.00%");
        assert_eq!(fb.descent_override, "20.00%");
        assert_eq!(fb.line_gap_override, "10.00%");
        assert_eq!(fb.local, "Times New Roman");
    }

    #[test]
    fn percent_formats_two_decimals() {
        assert_eq!(percent(0.9502), "95.02%");
        assert_eq!(percent(1.0), "100.00%");
        assert_eq!(percent(0.0), "0.00%");
    }

    // Opportunistic real-font parse: validates the sfnt reader against a
    // macOS system font when present. Skipped in CI / on other platforms since
    // we cannot commit the binary.
    #[test]
    fn parses_a_real_system_font_when_available() {
        let candidates = [
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Geneva.ttf",
            "/System/Library/Fonts/Monaco.ttf",
        ];
        let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
            eprintln!("no system font available; skipping real-font parse test");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let metrics = parse_sfnt_metrics(&bytes).unwrap();
        assert!(metrics.units_per_em > 0);
        assert!(metrics.ascent > 0);
        assert!(metrics.descent <= 0);
        // A text font should resolve an average lowercase width.
        assert!(
            metrics.x_width_avg.is_some(),
            "expected to measure xWidthAvg from {path}"
        );
        let x = metrics.x_width_avg.unwrap();
        assert!(x > 0.0 && x < f64::from(metrics.units_per_em));
    }
}
