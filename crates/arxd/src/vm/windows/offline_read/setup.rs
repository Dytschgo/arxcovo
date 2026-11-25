use std::collections::BTreeSet;

use super::{Budget, GENERALIZED, Invalid, Result, bytes, le16, le32};

const PAGE: usize = 4096;
const MAX_KEYS: usize = 24_576;
const MAX_NAME: usize = 512;

pub(super) const HIVE_LIMIT: u64 = 192 * (1 << 20);
pub(super) const INI_LIMIT: u64 = 4 * 1024;

struct Hive<'a> {
    bins: &'a [u8],
    /// Each allocated cell starts on an 8-byte boundary. Index the boundaries,
    /// so a supplied offset cannot use apparent cell headers inside payloads.
    allocated: Vec<bool>,
    root: u32,
}

impl<'a> Hive<'a> {
    fn parse(contents: &'a [u8], budget: &mut Budget) -> Result<Self> {
        budget.check_deadline()?;
        let base = bytes(contents, 0, PAGE)?;
        let len = le32(base, 40)? as usize;
        let checksum = base[..508]
            .chunks_exact(4)
            .try_fold(0u32, |check, word| le32(word, 0).map(|n| check ^ n))?;
        let checksum = match checksum {
            0 => 1,
            u32::MAX => u32::MAX - 1,
            n => n,
        };
        if contents.len() as u64 > HIVE_LIMIT
            || &base[..4] != b"regf"
            || le32(base, 4)? != le32(base, 8)?
            || le32(base, 20)? != 1
            || !(3..=6).contains(&le32(base, 24)?)
            || le32(base, 28)? != 0
            || le32(base, 32)? != 1
            || le32(base, 44)? != 1
            || le32(base, 508)? != checksum
            || !len.is_multiple_of(PAGE)
            || len == 0
        {
            return Err(Invalid);
        }
        let bins = bytes(contents, PAGE, len)?;
        budget.reserve_heap(len / 8)?;
        let mut allocated = vec![false; len / 8];
        let mut start = 0;
        while start < len {
            budget.spend_step(1)?;
            let header = bytes(bins, start, 32)?;
            let size = le32(header, 8)? as usize;
            let end = start
                .checked_add(size)
                .filter(|end| *end <= len)
                .ok_or(Invalid)?;
            if &header[..4] != b"hbin"
                || le32(header, 4)? as usize != start
                || size == 0
                || !size.is_multiple_of(PAGE)
            {
                return Err(Invalid);
            }
            let mut at = start + 32;
            while at < end {
                budget.spend_step(1)?;
                let signed = le32(bins, at)? as i32;
                let size = signed.unsigned_abs() as usize;
                if size < 8 || !size.is_multiple_of(8) || size > end - at {
                    return Err(Invalid);
                }
                allocated[at / 8] = signed < 0;
                at += size;
            }
            start = end;
        }
        budget.check_deadline()?;
        Ok(Self {
            bins,
            allocated,
            root: le32(base, 36)?,
        })
    }

    fn cell(&self, offset: u32) -> Result<&'a [u8]> {
        let at = offset as usize;
        if !at.is_multiple_of(8) || self.allocated.get(at / 8) != Some(&true) {
            return Err(Invalid);
        }
        let size = (le32(self.bins, at)? as i32).unsigned_abs() as usize;
        bytes(self.bins, at + 4, size - 4)
    }

    fn key(&self, offset: u32) -> Result<&'a [u8]> {
        let cell = self.cell(offset)?;
        if bytes(cell, 0, 2)? != b"nk" || cell.len() < 76 {
            return Err(Invalid);
        }
        // Registry symbolic links cannot redirect this lookup.
        if le16(cell, 2)? & 0x10 != 0 {
            return Err(Invalid);
        }
        Ok(cell)
    }

    fn child(&self, parent: u32, wanted: &str, budget: &mut Budget) -> Result<u32> {
        budget.check_deadline()?;
        let key = self.key(parent)?;
        let count = le32(key, 20)? as usize;
        if !(1..=MAX_KEYS).contains(&count) {
            return Err(Invalid);
        }
        let list_offset = le32(key, 28)?;
        let list = self.cell(list_offset)?;
        let mut seen_lists = BTreeSet::from([list_offset]);
        let mut seen_children = BTreeSet::new();
        let mut found = None;
        let mut inspect = |leaf: &[u8], budget: &mut Budget| -> Result<()> {
            let stride = match bytes(leaf, 0, 2)? {
                b"lh" | b"lf" => 8,
                b"li" => 4,
                _ => return Err(Invalid),
            };
            let items = usize::from(le16(leaf, 2)?);
            if items == 0 || items > count - seen_children.len() {
                return Err(Invalid);
            }
            for entry in bytes(leaf, 4, items * stride)?.chunks_exact(stride) {
                budget.spend_step(1)?;
                let offset = le32(entry, 0)?;
                if offset == parent || !seen_children.insert(offset) {
                    return Err(Invalid);
                }
                let child = self.key(offset)?;
                let name = bytes(child, 76, usize::from(le16(child, 72)?))?;
                if le32(child, 16)? != parent {
                    return Err(Invalid);
                }
                if name_is(name, le16(child, 2)? & 0x20 != 0, wanted)?
                    && found.replace(offset).is_some()
                {
                    return Err(Invalid);
                }
            }
            Ok(())
        };
        if bytes(list, 0, 2)? == b"ri" {
            let leaves = usize::from(le16(list, 2)?);
            if leaves == 0 || leaves > count {
                return Err(Invalid);
            }
            for entry in bytes(list, 4, leaves * 4)?.chunks_exact(4) {
                budget.spend_step(1)?;
                let offset = le32(entry, 0)?;
                if !seen_lists.insert(offset) {
                    return Err(Invalid);
                }
                inspect(self.cell(offset)?, budget)?;
            }
        } else {
            inspect(list, budget)?;
        }
        if seen_children.len() != count {
            return Err(Invalid);
        }
        found.ok_or(Invalid)
    }

    fn image_state(&self, budget: &mut Budget) -> Result<bool> {
        let mut key = self.root;
        let mut path = BTreeSet::from([key]);
        for part in ["Microsoft", "Windows", "CurrentVersion", "Setup", "State"] {
            key = self.child(key, part, budget)?;
            if !path.insert(key) {
                return Err(Invalid);
            }
        }
        let node = self.key(key)?;
        let count = le32(node, 36)? as usize;
        if !(1..=MAX_KEYS).contains(&count) {
            return Err(Invalid);
        }
        let list_offset = le32(node, 40)?;
        let list = self.cell(list_offset)?;
        let mut seen = BTreeSet::new();
        let mut found = None;
        for entry in bytes(list, 0, count * 4)?.chunks_exact(4) {
            budget.spend_step(1)?;
            let offset = le32(entry, 0)?;
            if !seen.insert(offset) || path.contains(&offset) || offset == list_offset {
                return Err(Invalid);
            }
            let value = self.cell(offset)?;
            if bytes(value, 0, 2)? != b"vk" {
                return Err(Invalid);
            }
            let name = bytes(value, 20, usize::from(le16(value, 2)?))?;
            if !name_is(name, le16(value, 16)? & 1 != 0, "ImageState")? {
                continue;
            }
            let size = le32(value, 4)? as usize;
            let data_offset = le32(value, 8)?;
            if found.is_some()
                || le32(value, 12)? != 1 // REG_SZ
                || !(2..=MAX_NAME).contains(&size)
                || !size.is_multiple_of(2)
                || path.contains(&data_offset)
                || data_offset == offset
                || data_offset == list_offset
            {
                return Err(Invalid);
            }
            let data = bytes(self.cell(data_offset)?, 0, size)?;
            if data[size - 2..] != [0, 0] {
                return Err(Invalid);
            }
            let text = decode_utf16(&data[..size - 2])?;
            if text.is_empty() || text.contains('\0') {
                return Err(Invalid);
            }
            found = Some(text == GENERALIZED);
        }
        budget.check_deadline()?;
        found.ok_or(Invalid)
    }
}

fn name_is(name: &[u8], compressed: bool, wanted: &str) -> Result<bool> {
    if name.len() > MAX_NAME {
        return Err(Invalid);
    }
    if compressed {
        Ok(name.eq_ignore_ascii_case(wanted.as_bytes()))
    } else {
        Ok(decode_utf16(name)?.eq_ignore_ascii_case(wanted))
    }
}

fn decode_utf16(data: &[u8]) -> Result<String> {
    if !data.len().is_multiple_of(2) {
        return Err(Invalid);
    }
    let units = data
        .chunks_exact(2)
        .map(|word| u16::from_le_bytes([word[0], word[1]]));
    char::decode_utf16(units)
        .collect::<std::result::Result<String, _>>()
        .map_err(|_| Invalid)
}

pub(super) fn registry(contents: &[u8], budget: &mut Budget) -> Result<bool> {
    Hive::parse(contents, budget)?.image_state(budget)
}

pub(super) fn state_ini(contents: &[u8], budget: &mut Budget) -> Result<bool> {
    budget.check_deadline()?;
    if contents.len() as u64 > INI_LIMIT {
        return Err(Invalid);
    }
    let text = if let Some(utf16) = contents.strip_prefix(&[0xff, 0xfe]) {
        decode_utf16(utf16)?
    } else {
        let utf8 = contents
            .strip_prefix(&[0xef, 0xbb, 0xbf])
            .unwrap_or(contents);
        std::str::from_utf8(utf8).map_err(|_| Invalid)?.to_owned()
    };
    let mut state = false;
    let mut state_section = false;
    let mut result = None;
    for line in text.lines().map(str::trim) {
        budget.spend_step(1)?;
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let section = line
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .ok_or(Invalid)?;
            state = section.eq_ignore_ascii_case("State");
            if state && std::mem::replace(&mut state_section, true) {
                return Err(Invalid);
            }
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(Invalid)?;
        if state && key.trim().eq_ignore_ascii_case("ImageState") {
            let value = value.trim();
            if value.is_empty()
                || !value
                    .bytes()
                    .all(|ch| ch.is_ascii_uppercase() || ch == b'_')
            {
                return Err(Invalid);
            }
            if result.replace(value == GENERALIZED).is_some() {
                return Err(Invalid);
            }
        }
    }
    result.ok_or(Invalid)
}
