// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Minimal WOFF2 -> sfnt reconstruction, just enough for metric extraction.
//!
//! We do not reconstruct outlines. WOFF2 only ever transforms the `glyf` and
//! `loca` tables; every table we need for metrics (`head`, `hhea`, `OS/2`,
//! `maxp`, `hmtx`, `cmap`) is stored untransformed, so we can pull them
//! verbatim out of the Brotli-decompressed data stream and stitch them into a
//! small, valid sfnt that [`crate::metrics::parse_sfnt_metrics`] reads.
//!
//! Reference: W3C WOFF2 specification, sections 4 (header) and 5 (table
//! directory), and the Brotli-compressed font data block.

use std::io::Read as _;

use byteorder::{BigEndian, ByteOrder};

use crate::error::BuildError;

const WOFF2_SIGNATURE: u32 = 0x774F_4632; // 'wOF2'
const HEADER_LEN: usize = 48;

/// Index -> 4-byte tag table for the 6-bit "known tag" field of a table
/// directory entry (WOFF2 spec, Table 2). Index 63 signals an arbitrary tag.
#[rustfmt::skip]
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post",
    b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT",
    b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea",
    b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC", b"JSTF", b"MATH",
    b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar",
    b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop",
    b"trak", b"Zapf", b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill",
];

const TAG_GLYF: u32 = u32::from_be_bytes(*b"glyf");
const TAG_LOCA: u32 = u32::from_be_bytes(*b"loca");

/// Returns `true` if `data` looks like a WOFF2 container.
pub fn is_woff2(data: &[u8]) -> bool {
    data.len() >= 4 && BigEndian::read_u32(data) == WOFF2_SIGNATURE
}

struct DirEntry {
    tag: u32,
    orig_length: usize,
    /// Number of bytes this table occupies in the decompressed stream.
    stream_length: usize,
    transformed: bool,
}

/// Reconstruct a minimal sfnt (offset table + the untransformed metric tables)
/// from WOFF2 `data`. The result is suitable only for metric parsing.
pub fn to_sfnt_for_metrics(data: &[u8]) -> Result<Vec<u8>, BuildError> {
    if !is_woff2(data) {
        return Err(BuildError::UnsupportedContainer);
    }
    let flavor = read_u32(data, 4)?;
    let num_tables = read_u16(data, 12)? as usize;
    let total_compressed_size = read_u32(data, 20)? as usize;

    // Parse the table directory that immediately follows the header, tracking
    // each table's byte offset within the (yet to be decompressed) stream.
    let mut pos = HEADER_LEN;
    let mut entries = Vec::with_capacity(num_tables);
    let mut stream_offsets = Vec::with_capacity(num_tables);
    let mut running = 0usize;
    for _ in 0..num_tables {
        let flags = *data.get(pos).ok_or(BuildError::Truncated)?;
        pos += 1;
        let tag_index = (flags & 0x3F) as usize;
        let transform_version = (flags >> 6) & 0x03;
        let tag = if tag_index == 63 {
            let t = read_u32(data, pos)?;
            pos += 4;
            t
        } else {
            u32::from_be_bytes(*KNOWN_TAGS[tag_index])
        };
        let orig_length = read_base128(data, &mut pos)? as usize;

        // Only glyf/loca are ever transformed (version 0 = transform applied,
        // 3 = null). Any non-zero version on another table is also a transform
        // per spec and carries a transformLength; we skip such tables' bytes
        // but never need their content.
        let transformed = if tag == TAG_GLYF || tag == TAG_LOCA {
            transform_version == 0
        } else {
            transform_version != 0
        };
        let stream_length = if transformed {
            read_base128(data, &mut pos)? as usize
        } else {
            orig_length
        };

        stream_offsets.push(running);
        running = running
            .checked_add(stream_length)
            .ok_or(BuildError::Woff2("stream length overflow".into()))?;
        entries.push(DirEntry {
            tag,
            orig_length,
            stream_length,
            transformed,
        });
    }

    // The Brotli-compressed font data block follows the directory.
    let compressed = data
        .get(pos..pos + total_compressed_size)
        .ok_or(BuildError::Truncated)?;
    let stream = brotli_decompress(compressed)?;

    // Extract the untransformed tables we need for metrics.
    const WANTED: [&[u8; 4]; 6] = [b"head", b"hhea", b"OS/2", b"maxp", b"hmtx", b"cmap"];
    let wanted: Vec<u32> = WANTED.iter().map(|t| u32::from_be_bytes(**t)).collect();

    let mut tables: Vec<(u32, Vec<u8>)> = Vec::new();
    for (entry, &offset) in entries.iter().zip(&stream_offsets) {
        if entry.transformed || !wanted.contains(&entry.tag) {
            continue;
        }
        let slice = stream
            .get(offset..offset + entry.stream_length)
            .ok_or(BuildError::Woff2(
                "table exceeds decompressed stream".into(),
            ))?;
        // stream_length == orig_length for untransformed tables.
        debug_assert_eq!(entry.stream_length, entry.orig_length);
        tables.push((entry.tag, slice.to_vec()));
    }

    Ok(build_sfnt(flavor, tables))
}

/// Assemble an sfnt from `flavor` and a set of `(tag, data)` tables. Directory
/// is tag-sorted; table data is 4-byte aligned. Checksums are left zero, which
/// metric parsing does not verify.
fn build_sfnt(flavor: u32, mut tables: Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let num_tables = tables.len();

    let entry_selector = if num_tables == 0 {
        0
    } else {
        (usize::BITS - 1 - num_tables.leading_zeros()) as u16
    };
    let search_range = (1u32 << entry_selector) * 16;
    let range_shift = (num_tables as u32) * 16 - search_range;

    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes()[2..]); // u16
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes()[2..]); // u16

    let mut data_offset = 12 + 16 * num_tables;
    let mut table_data = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum (unverified)
        out.extend_from_slice(&(data_offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());

        table_data.extend_from_slice(data);
        let pad = (4 - (data.len() % 4)) % 4;
        table_data.resize(table_data.len() + pad, 0u8);
        data_offset += data.len() + pad;
    }
    out.extend_from_slice(&table_data);
    out
}

fn brotli_decompress(compressed: &[u8]) -> Result<Vec<u8>, BuildError> {
    let mut out = Vec::new();
    brotli::Decompressor::new(compressed, 4096)
        .read_to_end(&mut out)
        .map_err(|e| BuildError::Woff2(format!("brotli: {e}")))?;
    Ok(out)
}

/// Decode a WOFF2 `UIntBase128` variable-length integer.
fn read_base128(data: &[u8], pos: &mut usize) -> Result<u32, BuildError> {
    let mut accum: u32 = 0;
    for i in 0..5 {
        let b = *data.get(*pos).ok_or(BuildError::Truncated)?;
        *pos += 1;
        // A leading 0x80 (continuation bit set, value bits zero) is disallowed.
        if i == 0 && b == 0x80 {
            return Err(BuildError::Woff2("base128 leading 0x80".into()));
        }
        // Reject values that would overflow u32.
        if accum & 0xFE00_0000 != 0 {
            return Err(BuildError::Woff2("base128 overflow".into()));
        }
        accum = (accum << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            return Ok(accum);
        }
    }
    Err(BuildError::Woff2("base128 too long".into()))
}

fn read_u16(data: &[u8], at: usize) -> Result<u16, BuildError> {
    data.get(at..at + 2)
        .map(BigEndian::read_u16)
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
    fn detects_woff2_signature() {
        assert!(is_woff2(b"wOF2 rest of header..."));
        assert!(!is_woff2(b"wOFF")); // woff1
        assert!(!is_woff2(&[0x00, 0x01, 0x00, 0x00])); // sfnt
        assert!(!is_woff2(b"no"));
    }

    #[test]
    fn base128_decodes_single_and_multi_byte() {
        let mut pos = 0;
        assert_eq!(read_base128(&[0x3F], &mut pos).unwrap(), 63);
        assert_eq!(pos, 1);

        // 0x8F 0x00 = (0x0F << 7) | 0x00 = 1920
        let mut pos = 0;
        assert_eq!(read_base128(&[0x8F, 0x00], &mut pos).unwrap(), 1920);
        assert_eq!(pos, 2);
    }

    #[test]
    fn base128_rejects_leading_0x80_and_overflow() {
        let mut pos = 0;
        assert!(read_base128(&[0x80, 0x01], &mut pos).is_err());
        let mut pos = 0;
        // Six continuation bytes overflow the 5-byte maximum.
        assert!(read_base128(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F], &mut pos).is_err());
    }

    #[test]
    fn build_sfnt_lays_out_offset_table_and_records() {
        let tables = vec![
            (u32::from_be_bytes(*b"head"), vec![1u8, 2, 3]),
            (u32::from_be_bytes(*b"hhea"), vec![4u8, 5, 6, 7]),
        ];
        let sfnt = build_sfnt(0x0001_0000, tables);
        // num tables
        assert_eq!(BigEndian::read_u16(&sfnt[4..]), 2);
        // First record tag is 'head' (tag-sorted; 'head' < 'hhea').
        assert_eq!(&sfnt[12..16], b"head");
        let head_off = BigEndian::read_u32(&sfnt[20..]) as usize;
        let head_len = BigEndian::read_u32(&sfnt[24..]) as usize;
        assert_eq!(head_len, 3);
        assert_eq!(&sfnt[head_off..head_off + 3], &[1, 2, 3]);
        // Data region begins after the 12-byte offset table + 2*16 records.
        assert_eq!(head_off, 12 + 32);
    }

    // Round-trips a real WOFF2 through decode + metric parse when a system
    // WOFF2 is available. macOS ships none by default, so this is usually a
    // skip; it exercises the full path where a corpus font exists.
    #[test]
    fn decodes_a_real_woff2_when_available() {
        let candidates = [
            "/System/Library/Fonts/Supplemental/fonts/Geist.woff2", // unlikely
        ];
        let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
            eprintln!("no system woff2 available; skipping woff2 decode test");
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let sfnt = to_sfnt_for_metrics(&bytes).unwrap();
        let metrics = crate::metrics::parse_sfnt_metrics(&sfnt).unwrap();
        assert!(metrics.units_per_em > 0);
    }
}
