//! Cell-level parsing: nk (key), vk (value), subkey lists, value lists.

use super::value::{Value, ValueType};
use super::{BASE_BLOCK_LEN, Hive, HiveError, MAX_DEPTH, Result, u16_at, u32_at};

const NK_COMP_NAME: u16 = 0x0020;
const VK_COMP_NAME: u16 = 0x0001;
const VK_INLINE: u32 = 0x8000_0000;
/// Values larger than this are stored in `db` big-data cells (hive >= 1.4).
const BIG_DATA_THRESHOLD: usize = 16344;

/// Return the payload of an allocated cell (without the 4-byte size field).
pub(super) fn cell(hive: &Hive, offset: u32) -> Result<&[u8]> {
    let start = BASE_BLOCK_LEN
        .checked_add(offset as usize)
        .ok_or(HiveError::BadCell(offset))?;
    let end_limit = hive.bins_end();
    if offset == u32::MAX || start + 4 > end_limit {
        return Err(HiveError::BadCell(offset));
    }
    let raw = u32_at(&hive.data, start).ok_or(HiveError::BadCell(offset))? as i32;
    // Allocated cells have a negative size.
    if raw >= 0 {
        return Err(HiveError::BadCell(offset));
    }
    let len = raw.unsigned_abs() as usize;
    if len < 4 || start + len > end_limit {
        return Err(HiveError::BadCell(offset));
    }
    Ok(&hive.data[start + 4..start + len])
}

fn sig<'h>(hive: &'h Hive, offset: u32, expected: &'static str) -> Result<&'h [u8]> {
    let c = cell(hive, offset)?;
    if c.len() < 2 || &c[0..2] != expected.as_bytes() {
        return Err(HiveError::BadSignature { expected, offset });
    }
    Ok(c)
}

fn malformed(what: &str, offset: u32) -> HiveError {
    HiveError::Malformed(format!("{what} at cell {offset:#x}"))
}

pub(super) fn decode_name(bytes: &[u8], compressed: bool) -> String {
    if compressed {
        bytes.iter().map(|&b| b as char).collect() // Latin-1
    } else {
        utf16le(bytes)
    }
}

pub(super) fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[derive(Debug, Clone)]
pub(super) struct Nk {
    pub name: String,
    pub subkey_count: u32,
    pub subkeys_offset: u32,
    pub value_count: u32,
    pub values_offset: u32,
}

impl Nk {
    pub fn parse(hive: &Hive, offset: u32) -> Result<Self> {
        let c = sig(hive, offset, "nk")?;
        let f = |at| u32_at(c, at).ok_or_else(|| malformed("short nk", offset));
        let flags = u16_at(c, 2).ok_or_else(|| malformed("short nk", offset))?;
        let name_len = u16_at(c, 72).ok_or_else(|| malformed("short nk", offset))? as usize;
        let name_bytes = c
            .get(76..76 + name_len)
            .ok_or_else(|| malformed("nk name past cell end", offset))?;
        Ok(Nk {
            name: decode_name(name_bytes, flags & NK_COMP_NAME != 0),
            subkey_count: f(20)?,
            subkeys_offset: f(28)?,
            value_count: f(36)?,
            values_offset: f(40)?,
        })
    }
}

/// Gather nk offsets from an lf/lh/li list, recursing through ri indexes.
pub(super) fn collect_subkey_offsets(
    hive: &Hive,
    list: u32,
    depth: usize,
    out: &mut Vec<u32>,
) -> Result<()> {
    if depth > 2 || out.len() > hive.data.len() / 8 {
        return Err(HiveError::TooDeep);
    }
    let c = cell(hive, list)?;
    let kind = c.get(0..2).ok_or_else(|| malformed("short list", list))?;
    let count = u16_at(c, 2).ok_or_else(|| malformed("short list", list))? as usize;
    let stride = match kind {
        b"lf" | b"lh" => 8,
        b"li" | b"ri" => 4,
        _ => {
            return Err(HiveError::BadSignature {
                expected: "lf/lh/li/ri",
                offset: list,
            });
        }
    };
    for i in 0..count {
        let off = u32_at(c, 4 + i * stride).ok_or_else(|| malformed("list past cell end", list))?;
        if kind == b"ri" {
            collect_subkey_offsets(hive, off, depth + 1, out)?;
        } else {
            out.push(off);
        }
    }
    Ok(())
}

pub(super) fn read_values(hive: &Hive, count: u32, list: u32) -> Result<Vec<Value>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let c = cell(hive, list)?;
    if count as usize > c.len() / 4 || count as usize > MAX_DEPTH * 64 {
        return Err(malformed("value count exceeds list", list));
    }
    (0..count as usize)
        .map(|i| {
            let off = u32_at(c, i * 4).ok_or_else(|| malformed("value list", list))?;
            read_vk(hive, off)
        })
        .collect()
}

fn read_vk(hive: &Hive, offset: u32) -> Result<Value> {
    let c = sig(hive, offset, "vk")?;
    let short = || malformed("short vk", offset);
    let name_len = u16_at(c, 2).ok_or_else(short)? as usize;
    let size_raw = u32_at(c, 4).ok_or_else(short)?;
    let data_off = u32_at(c, 8).ok_or_else(short)?;
    let vtype = u32_at(c, 12).ok_or_else(short)?;
    let flags = u16_at(c, 16).ok_or_else(short)?;
    let name_bytes = c.get(20..20 + name_len).ok_or_else(short)?;
    let name = decode_name(name_bytes, flags & VK_COMP_NAME != 0);

    let data = if size_raw & VK_INLINE != 0 {
        let n = ((size_raw & !VK_INLINE) as usize).min(4);
        c[8..12][..n].to_vec()
    } else {
        read_data(hive, data_off, size_raw as usize)?
    };
    Ok(Value {
        name,
        kind: ValueType::from(vtype),
        data,
    })
}

fn read_data(hive: &Hive, offset: u32, size: usize) -> Result<Vec<u8>> {
    if size == 0 {
        return Ok(Vec::new());
    }
    let c = cell(hive, offset)?;
    if size > BIG_DATA_THRESHOLD && hive.header.minor >= 4 && c.get(0..2) == Some(b"db") {
        return read_big_data(hive, offset, c, size);
    }
    c.get(..size)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| malformed("value data past cell end", offset))
}

fn read_big_data(hive: &Hive, offset: u32, db: &[u8], size: usize) -> Result<Vec<u8>> {
    let segs = u16_at(db, 2).ok_or_else(|| malformed("short db", offset))? as usize;
    let list_off = u32_at(db, 4).ok_or_else(|| malformed("short db", offset))?;
    let list = cell(hive, list_off)?;
    let mut out = Vec::with_capacity(size.min(hive.data.len()));
    for i in 0..segs {
        let seg_off = u32_at(list, i * 4).ok_or_else(|| malformed("db segment list", list_off))?;
        let seg = cell(hive, seg_off)?;
        let take = (size - out.len()).min(BIG_DATA_THRESHOLD).min(seg.len());
        out.extend_from_slice(&seg[..take]);
        if out.len() == size {
            break;
        }
    }
    if out.len() != size {
        return Err(malformed("db data shorter than declared", offset));
    }
    Ok(out)
}
