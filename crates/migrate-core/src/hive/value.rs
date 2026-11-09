//! Registry value types and data decoding.

use super::cell::utf16le;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    None,
    Sz,
    ExpandSz,
    Binary,
    Dword,
    DwordBigEndian,
    Link,
    MultiSz,
    Qword,
    Other(u32),
}

impl From<u32> for ValueType {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Sz,
            2 => Self::ExpandSz,
            3 => Self::Binary,
            4 => Self::Dword,
            5 => Self::DwordBigEndian,
            6 => Self::Link,
            7 => Self::MultiSz,
            11 => Self::Qword,
            n => Self::Other(n),
        }
    }
}

impl From<ValueType> for u32 {
    fn from(v: ValueType) -> u32 {
        match v {
            ValueType::None => 0,
            ValueType::Sz => 1,
            ValueType::ExpandSz => 2,
            ValueType::Binary => 3,
            ValueType::Dword => 4,
            ValueType::DwordBigEndian => 5,
            ValueType::Link => 6,
            ValueType::MultiSz => 7,
            ValueType::Qword => 11,
            ValueType::Other(n) => n,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub name: String,
    pub kind: ValueType,
    pub data: Vec<u8>,
}

/// Decode UTF-16LE up to the first NUL.
fn utf16_until_nul(data: &[u8]) -> String {
    let end = data
        .chunks_exact(2)
        .position(|c| c == [0, 0])
        .map_or(data.len() & !1, |i| i * 2);
    utf16le(&data[..end])
}

impl Value {
    /// REG_SZ / REG_EXPAND_SZ as text (unexpanded). Other types → `None`.
    pub fn as_string(&self) -> Option<String> {
        match self.kind {
            ValueType::Sz | ValueType::ExpandSz => Some(utf16_until_nul(&self.data)),
            _ => None,
        }
    }

    pub fn as_dword(&self) -> Option<u32> {
        let b: [u8; 4] = self.data.get(..4)?.try_into().ok()?;
        match self.kind {
            ValueType::Dword => Some(u32::from_le_bytes(b)),
            ValueType::DwordBigEndian => Some(u32::from_be_bytes(b)),
            _ => None,
        }
    }

    pub fn as_multi_sz(&self) -> Option<Vec<String>> {
        if self.kind != ValueType::MultiSz {
            return None;
        }
        let text = utf16le(&self.data[..self.data.len() & !1]);
        Some(
            text.split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }
}
