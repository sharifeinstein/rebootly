//! Minimal regf *writer*, used only to build deterministic test fixtures.
//!
//! The tool never writes hives to a source disk; this produces new files for
//! `tests/fixtures` and unit tests. Output is a single hive bin, sorted
//! `lh` subkey lists, and no security cells (readers here don't need them).

use std::collections::BTreeMap;

use super::value::ValueType;
use super::{BASE_BLOCK_LEN, base_block_checksum};

#[derive(Debug, Default, Clone)]
pub struct WKey {
    subkeys: BTreeMap<String, (String, WKey)>, // uppercase name -> (name, key)
    values: Vec<(String, u32, Vec<u8>)>,
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

impl WKey {
    /// Get or create a descendant by backslash path.
    pub fn key(&mut self, path: &str) -> &mut WKey {
        let mut cur = self;
        for part in path.split('\\').filter(|p| !p.is_empty()) {
            cur = &mut cur
                .subkeys
                .entry(part.to_uppercase())
                .or_insert_with(|| (part.to_owned(), WKey::default()))
                .1;
        }
        cur
    }

    pub fn set(&mut self, name: &str, kind: ValueType, data: Vec<u8>) -> &mut Self {
        self.values
            .retain(|(n, _, _)| !n.eq_ignore_ascii_case(name));
        self.values.push((name.to_owned(), kind.into(), data));
        self
    }
    pub fn sz(&mut self, name: &str, v: &str) -> &mut Self {
        self.set(name, ValueType::Sz, utf16z(v))
    }
    pub fn expand_sz(&mut self, name: &str, v: &str) -> &mut Self {
        self.set(name, ValueType::ExpandSz, utf16z(v))
    }
    pub fn dword(&mut self, name: &str, v: u32) -> &mut Self {
        self.set(name, ValueType::Dword, v.to_le_bytes().to_vec())
    }
}

#[derive(Debug, Clone)]
pub struct HiveWriter {
    root_name: String,
    pub root: WKey,
    primary_seq: u32,
    secondary_seq: u32,
}

impl HiveWriter {
    pub fn new(root_name: &str) -> Self {
        HiveWriter {
            root_name: root_name.into(),
            root: WKey::default(),
            primary_seq: 1,
            secondary_seq: 1,
        }
    }

    pub fn key(&mut self, path: &str) -> &mut WKey {
        self.root.key(path)
    }

    /// Set base block sequence numbers (unequal = dirty fixture).
    pub fn sequences(&mut self, primary: u32, secondary: u32) -> &mut Self {
        self.primary_seq = primary;
        self.secondary_seq = secondary;
        self
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bins = Bins { buf: vec![0; 32] };
        let root = bins.write_key(&self.root_name, &self.root, 0xFFFF_FFFF, true);
        bins.finish();
        let mut out = vec![0u8; BASE_BLOCK_LEN];
        let mut put = |at: usize, v: u32| out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(4, self.primary_seq);
        put(8, self.secondary_seq);
        put(20, 1); // major
        put(24, 5); // minor
        put(32, 1); // format: direct memory load
        put(36, root);
        put(40, bins.buf.len() as u32);
        put(44, 1); // clustering factor
        out[0..4].copy_from_slice(b"regf");
        let sum = base_block_checksum(&out);
        out[508..512].copy_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&bins.buf);
        out
    }

    /// A Windows 8.1-style `.LOG1` holding one entry at `seq` (no page data).
    /// With `seq >= secondary` the hive is reported as having pending logs.
    pub fn pending_log_bytes(&self, seq: u32) -> Vec<u8> {
        let base = self.to_bytes();
        let mut log = base[..512].to_vec();
        let mut entry = vec![0u8; 512];
        entry[0..4].copy_from_slice(b"HvLE");
        entry[4..8].copy_from_slice(&512u32.to_le_bytes());
        entry[12..16].copy_from_slice(&seq.to_le_bytes());
        log.extend_from_slice(&entry);
        log
    }
}

struct Bins {
    buf: Vec<u8>,
}

/// `lh` name hash: uppercase, h = h*37 + c.
fn lh_hash(name: &str) -> u32 {
    name.to_uppercase()
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(37).wrapping_add(c as u32))
}

impl Bins {
    /// Allocate a cell holding `payload`; returns its offset.
    fn alloc(&mut self, payload: &[u8]) -> u32 {
        let off = self.buf.len() as u32;
        let len = (payload.len() + 4).div_ceil(8) * 8;
        self.buf.extend_from_slice(&(-(len as i32)).to_le_bytes());
        self.buf.extend_from_slice(payload);
        self.buf.resize(off as usize + len, 0);
        off
    }

    fn patch(&mut self, cell: u32, field: usize, v: u32) {
        let at = cell as usize + 4 + field;
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn write_key(&mut self, name: &str, key: &WKey, parent: u32, root: bool) -> u32 {
        let latin1 = name.chars().all(|c| (c as u32) < 256);
        let name_bytes: Vec<u8> = if latin1 {
            name.chars().map(|c| c as u8).collect()
        } else {
            name.encode_utf16().flat_map(u16::to_le_bytes).collect()
        };
        let mut nk = vec![0u8; 76];
        nk[0..2].copy_from_slice(b"nk");
        let flags: u16 = if latin1 { 0x20 } else { 0 } | if root { 0x0C } else { 0 };
        nk[2..4].copy_from_slice(&flags.to_le_bytes());
        nk[16..20].copy_from_slice(&parent.to_le_bytes());
        for at in [28, 32, 40, 44, 48] {
            nk[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        nk[72..74].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        nk.extend_from_slice(&name_bytes);
        let me = self.alloc(&nk);

        if !key.values.is_empty() {
            let offs: Vec<u32> = key.values.iter().map(|v| self.write_value(v)).collect();
            let list: Vec<u8> = offs.iter().flat_map(|o| o.to_le_bytes()).collect();
            let l = self.alloc(&list);
            self.patch(me, 36, offs.len() as u32);
            self.patch(me, 40, l);
        }
        if !key.subkeys.is_empty() {
            let mut lh = Vec::new();
            for (sub_name, sub) in key.subkeys.values() {
                let off = self.write_key(sub_name, sub, me, false);
                lh.extend_from_slice(&off.to_le_bytes());
                lh.extend_from_slice(&lh_hash(sub_name).to_le_bytes());
            }
            let mut list = b"lh".to_vec();
            list.extend_from_slice(&(key.subkeys.len() as u16).to_le_bytes());
            list.extend_from_slice(&lh);
            let l = self.alloc(&list);
            self.patch(me, 20, key.subkeys.len() as u32);
            self.patch(me, 28, l);
        }
        me
    }

    fn write_value(&mut self, (name, kind, data): &(String, u32, Vec<u8>)) -> u32 {
        assert!(
            data.len() <= 16344,
            "fixture writer does not emit big-data cells"
        );
        let (size, data_off) = if data.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..data.len()].copy_from_slice(data);
            (data.len() as u32 | 0x8000_0000, u32::from_le_bytes(inline))
        } else {
            (data.len() as u32, self.alloc(data))
        };
        let mut vk = vec![0u8; 20];
        vk[0..2].copy_from_slice(b"vk");
        vk[2..4].copy_from_slice(&(name.len() as u16).to_le_bytes());
        vk[4..8].copy_from_slice(&size.to_le_bytes());
        vk[8..12].copy_from_slice(&data_off.to_le_bytes());
        vk[12..16].copy_from_slice(&kind.to_le_bytes());
        vk[16..18].copy_from_slice(&1u16.to_le_bytes()); // ASCII name
        assert!(
            name.is_ascii(),
            "fixture writer emits ASCII value names only"
        );
        vk.extend_from_slice(name.as_bytes());
        self.alloc(&vk)
    }

    /// Write the hbin header and pad to 4 KiB with one free cell.
    fn finish(&mut self) {
        let used = self.buf.len();
        let total = (used + 8).div_ceil(4096) * 4096;
        let free = (total - used) as i32;
        self.buf.extend_from_slice(&free.to_le_bytes());
        self.buf.resize(total, 0);
        self.buf[0..4].copy_from_slice(b"hbin");
        self.buf[8..12].copy_from_slice(&(total as u32).to_le_bytes());
    }
}
