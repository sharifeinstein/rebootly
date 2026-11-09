//! Read-only parser for Windows registry hive files (regf format).
//!
//! Our own implementation (nt-hive is GPL-2.0-or-later; see PROGRESS.md
//! decisions). Every offset read from the file is bounds-checked: malformed
//! input yields `HiveError`, never a panic.

mod cell;
pub mod dirty;
mod value;
pub mod writer;

use std::path::Path;

pub use value::{Value, ValueType};

const BASE_BLOCK_LEN: usize = 4096;
/// Max nesting for `ri` index lists and path lookups; guards against cycles.
const MAX_DEPTH: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum HiveError {
    #[error("i/o error reading hive: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a registry hive (missing 'regf' signature)")]
    NotAHive,
    #[error("hive truncated or malformed: {0}")]
    Malformed(String),
    #[error("cell offset {0:#x} is out of bounds or not allocated")]
    BadCell(u32),
    #[error("expected '{expected}' signature at cell {offset:#x}")]
    BadSignature { expected: &'static str, offset: u32 },
    #[error("hive nesting too deep (possible cycle)")]
    TooDeep,
}

pub type Result<T> = std::result::Result<T, HiveError>;

/// Fields of the hive base block that we use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub primary_seq: u32,
    pub secondary_seq: u32,
    pub major: u32,
    pub minor: u32,
    pub root_offset: u32,
    pub bins_size: u32,
    pub checksum_ok: bool,
}

impl Header {
    /// A hive whose sequence numbers differ was not cleanly written.
    pub fn seq_mismatch(&self) -> bool {
        self.primary_seq != self.secondary_seq
    }
}

/// An in-memory hive. Cheap to query; keys borrow from it.
pub struct Hive {
    data: Vec<u8>,
    header: Header,
}

impl std::fmt::Debug for Hive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hive")
            .field("header", &self.header)
            .finish()
    }
}

pub(crate) fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

pub(crate) fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// XOR of the first 127 dwords, with the two reserved results remapped.
pub(crate) fn base_block_checksum(block: &[u8]) -> u32 {
    let mut sum = 0u32;
    for i in 0..127 {
        sum ^= u32_at(block, i * 4).unwrap_or(0);
    }
    match sum {
        0 => 1,
        u32::MAX => u32::MAX - 1,
        s => s,
    }
}

impl Hive {
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        if data.len() < BASE_BLOCK_LEN || &data[0..4] != b"regf" {
            return Err(HiveError::NotAHive);
        }
        let h = |at| u32_at(&data, at).unwrap_or(0);
        let header = Header {
            primary_seq: h(4),
            secondary_seq: h(8),
            major: h(20),
            minor: h(24),
            root_offset: h(36),
            bins_size: h(40),
            checksum_ok: base_block_checksum(&data) == h(508),
        };
        if header.major != 1 {
            return Err(HiveError::Malformed(format!(
                "unsupported major version {}",
                header.major
            )));
        }
        let hive = Hive { data, header };
        hive.root()?; // validate the root key up front
        Ok(hive)
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// End of the hive-bins region, clamped to what is actually in the file.
    fn bins_end(&self) -> usize {
        let declared = BASE_BLOCK_LEN.saturating_add(self.header.bins_size as usize);
        declared.min(self.data.len())
    }

    pub fn root(&self) -> Result<Key<'_>> {
        Key::load(self, self.header.root_offset)
    }

    /// Open a key by backslash-separated path relative to the root.
    pub fn open_key(&self, path: &str) -> Result<Option<Key<'_>>> {
        self.root()?.open(path)
    }
}

/// A registry key inside a `Hive`.
#[derive(Clone)]
pub struct Key<'h> {
    hive: &'h Hive,
    nk: cell::Nk,
}

impl std::fmt::Debug for Key<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key").field("name", &self.nk.name).finish()
    }
}

impl<'h> Key<'h> {
    fn load(hive: &'h Hive, offset: u32) -> Result<Self> {
        Ok(Key {
            hive,
            nk: cell::Nk::parse(hive, offset)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.nk.name
    }

    pub fn subkeys(&self) -> Result<Vec<Key<'h>>> {
        if self.nk.subkey_count == 0 {
            return Ok(Vec::new());
        }
        let mut offs = Vec::new();
        cell::collect_subkey_offsets(self.hive, self.nk.subkeys_offset, 0, &mut offs)?;
        offs.into_iter().map(|o| Key::load(self.hive, o)).collect()
    }

    /// Case-insensitive direct child lookup.
    pub fn subkey(&self, name: &str) -> Result<Option<Key<'h>>> {
        Ok(self.subkeys()?.into_iter().find(|k| eq_ci(k.name(), name)))
    }

    /// Walk a backslash-separated relative path (empty components ignored).
    pub fn open(&self, path: &str) -> Result<Option<Key<'h>>> {
        let mut cur = self.clone();
        for (depth, part) in path.split('\\').filter(|p| !p.is_empty()).enumerate() {
            if depth > MAX_DEPTH {
                return Err(HiveError::TooDeep);
            }
            match cur.subkey(part)? {
                Some(k) => cur = k,
                None => return Ok(None),
            }
        }
        Ok(Some(cur))
    }

    pub fn values(&self) -> Result<Vec<Value>> {
        cell::read_values(self.hive, self.nk.value_count, self.nk.values_offset)
    }

    /// Case-insensitive value lookup; `""` is the default value.
    pub fn value(&self, name: &str) -> Result<Option<Value>> {
        Ok(self.values()?.into_iter().find(|v| eq_ci(&v.name, name)))
    }

    /// Convenience: string value (REG_SZ / REG_EXPAND_SZ), else `None`.
    pub fn string(&self, name: &str) -> Result<Option<String>> {
        Ok(self.value(name)?.and_then(|v| v.as_string()))
    }
}

/// Registry names compare case-insensitively.
pub fn eq_ci(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_uppercase)
        .eq(b.chars().flat_map(char::to_uppercase))
}

#[cfg(test)]
mod tests;
