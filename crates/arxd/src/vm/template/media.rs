//! Fixed ISO9660/Joliet seed profiles. Both namespaces share file extents.

use std::io::{self, Write};

const SECTOR: usize = 2048;
const PRIMARY: u32 = 16;
const PRIMARY_ROOT: u32 = 23;
const JOLIET_ROOT: u32 = 24;
const MAX_SECTORS: u32 = (64 * 1024 * 1024 / SECTOR) as u32;
pub(super) const MAX_GUEST: usize = 32 * 1024 * 1024;
const ZERO: [u8; SECTOR] = [0; SECTOR];

// Each identifier includes the extension separator and version, even when
// its extension is empty. Directory order compares base name, then extension.
const PRIMARY_NAMES: [&str; 7] = [
    "ARXAUDIT.;1",
    "ARXAUDIT.SVC;1",
    "ARXGUEST.;1",
    "ARXGUEST.SVC;1",
    "FINALIZE.;1",
    "METADATA.;1",
    "USERDATA.;1",
];
const JOLIET_NAMES: [&str; 7] = [
    "arx-account-audit.;1",
    "arx-account-audit.service;1",
    "arx-guest.;1",
    "arx-guest.service;1",
    "arx-finalize.;1",
    "meta-data.;1",
    "user-data.;1",
];

#[derive(Clone, Copy)]
struct Extent {
    sector: u32,
    length: u32,
}

pub(super) struct Image<'a, const N: usize = 7> {
    files: [&'a [u8]; N],
    extents: [Extent; N],
    sectors: u32,
    label: &'static str,
    primary: [&'static str; N],
    joliet: [&'static str; N],
    joliet_order: [usize; N],
}

impl<'a> Image<'a> {
    pub(super) fn new(files: [&'a [u8]; 7]) -> Option<Self> {
        let lengths = files.map(<[u8]>::len);
        let (extents, sectors) = layout(lengths)?;
        Some(Self {
            files,
            extents,
            sectors,
            label: "CIDATA",
            primary: PRIMARY_NAMES,
            joliet: JOLIET_NAMES,
            joliet_order: [0, 1, 4, 2, 3, 5, 6],
        })
    }
}

impl<'a> Image<'a, 5> {
    pub(super) fn windows(files: [&'a [u8]; 5]) -> Option<Self> {
        let lengths = files.map(<[u8]>::len);
        for (length, limit) in lengths
            .into_iter()
            .zip([MAX_GUEST, 1024, 128, 32 * 1024, 16 * 1024])
        {
            if length > limit {
                return None;
            }
        }
        let (extents, sectors) = file_extents(lengths)?;
        Some(Self {
            files,
            extents,
            sectors,
            label: "ARXSETUP",
            primary: [
                "ARXGUEST.EXE;1",
                "ARXPINS.TXT;1",
                "ARXSEED.ID;1",
                "ARXSETUP.PS1;1",
                "AUTOUNAT.XML;1",
            ],
            joliet: [
                "arxguest.exe;1",
                "arxpins.txt;1",
                "arxseed.id;1",
                "arxsetup.ps1;1",
                "Autounattend.xml;1",
            ],
            joliet_order: [0, 1, 2, 3, 4],
        })
    }
}

impl<const N: usize> Image<'_, N> {
    pub(super) fn write(&self, mut out: impl Write) -> io::Result<()> {
        for _ in 0..PRIMARY {
            out.write_all(&ZERO)?;
        }
        out.write_all(&descriptor(false, self.sectors, self.label))?;
        out.write_all(&descriptor(true, self.sectors, self.label))?;
        let mut end = ZERO;
        end[..7].copy_from_slice(b"\xffCD001\x01");
        out.write_all(&end)?;
        for root in [PRIMARY_ROOT, JOLIET_ROOT] {
            out.write_all(&path_table(root, false))?;
            out.write_all(&path_table(root, true))?;
        }
        for (root, joliet) in [(PRIMARY_ROOT, false), (JOLIET_ROOT, true)] {
            out.write_all(&self.directory(root, joliet))?;
        }
        for file in self.files {
            out.write_all(file)?;
            let padding = (SECTOR - file.len() % SECTOR) % SECTOR;
            out.write_all(&ZERO[..padding])?;
        }
        Ok(())
    }

    fn directory(&self, sector: u32, joliet: bool) -> [u8; SECTOR] {
        let root = root_extent(sector);
        let mut data = ZERO;
        let mut offset = record(&mut data, root, b"\0", true);
        offset += record(&mut data[offset..], root, b"\x01", true);
        let order = if joliet {
            self.joliet_order
        } else {
            std::array::from_fn(|index| index)
        };
        for index in order {
            let name = if joliet {
                self.joliet[index]
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>()
            } else {
                self.primary[index].as_bytes().to_vec()
            };
            offset += record(&mut data[offset..], self.extents[index], &name, false);
        }
        data
    }
}

fn layout(lengths: [usize; 7]) -> Option<([Extent; 7], u32)> {
    let [_, _, guest, _, _, meta, user] = lengths;
    if guest > MAX_GUEST || meta > 1024 || user > 16 * 1024 {
        return None;
    }
    file_extents(lengths)
}

fn file_extents<const N: usize>(lengths: [usize; N]) -> Option<([Extent; N], u32)> {
    let mut sector = JOLIET_ROOT + 1;
    let mut extents = [Extent {
        sector: 0,
        length: 0,
    }; N];
    for (extent, length) in extents.iter_mut().zip(lengths) {
        let length = u32::try_from(length).ok()?;
        if length == 0 {
            return None;
        }
        *extent = Extent { sector, length };
        sector = sector.checked_add(length.div_ceil(SECTOR as u32))?;
    }
    (sector <= MAX_SECTORS).then_some((extents, sector))
}

fn both_u16(buf: &mut [u8], value: u16) {
    buf[..2].copy_from_slice(&value.to_le_bytes());
    buf[2..4].copy_from_slice(&value.to_be_bytes());
}

fn both_u32(buf: &mut [u8], value: u32) {
    buf[..4].copy_from_slice(&value.to_le_bytes());
    buf[4..8].copy_from_slice(&value.to_be_bytes());
}

fn record(buf: &mut [u8], extent: Extent, name: &[u8], directory: bool) -> usize {
    let length = (33 + name.len()).next_multiple_of(2);
    buf[0] = length as u8;
    both_u32(&mut buf[2..10], extent.sector);
    both_u32(&mut buf[10..18], extent.length);
    buf[18..25].copy_from_slice(&[100, 1, 1, 0, 0, 0, 0]); // 2000-01-01 UTC
    buf[25] = if directory { 2 } else { 0 };
    both_u16(&mut buf[28..32], 1);
    buf[32] = name.len() as u8;
    buf[33..33 + name.len()].copy_from_slice(name);
    length
}

fn root_extent(sector: u32) -> Extent {
    Extent {
        sector,
        length: SECTOR as u32,
    }
}

fn path_table(root: u32, big_endian: bool) -> [u8; SECTOR] {
    let mut block = ZERO;
    block[0] = 1;
    if big_endian {
        block[2..6].copy_from_slice(&root.to_be_bytes());
        block[6..8].copy_from_slice(&1_u16.to_be_bytes());
    } else {
        block[2..6].copy_from_slice(&root.to_le_bytes());
        block[6..8].copy_from_slice(&1_u16.to_le_bytes());
    }
    block
}

fn descriptor(joliet: bool, sectors: u32, label: &str) -> [u8; SECTOR] {
    let mut block = ZERO;
    block[0] = if joliet { 2 } else { 1 };
    block[1..7].copy_from_slice(b"CD001\x01");
    for field in [8..40, 40..72, 190..318, 318..446, 446..574, 574..702] {
        if joliet {
            for character in block[field].chunks_exact_mut(2) {
                character.copy_from_slice(b"\0 ");
            }
        } else {
            block[field].fill(b' ');
        }
    }
    if joliet {
        for (pair, byte) in block[40..72].chunks_exact_mut(2).zip(label.bytes()) {
            pair.copy_from_slice(&[0, byte]);
        }
        block[88..91].copy_from_slice(b"%/E");
    } else {
        block[40..40 + label.len()].copy_from_slice(label.as_bytes());
    }
    let (root, path): (u32, u32) = if joliet {
        (JOLIET_ROOT, 21)
    } else {
        (PRIMARY_ROOT, 19)
    };
    both_u32(&mut block[80..88], sectors);
    both_u16(&mut block[120..124], 1);
    both_u16(&mut block[124..128], 1);
    both_u16(&mut block[128..132], SECTOR as u16);
    both_u32(&mut block[132..140], 10);
    block[140..144].copy_from_slice(&path.to_le_bytes());
    block[148..152].copy_from_slice(&(path + 1).to_be_bytes());
    record(&mut block[156..190], root_extent(root), b"\0", true);
    // The Joliet fields follow libisofs: UCS-2 spaces and a trailing zero.
    // https://sources.debian.org/src/libisofs/1.5.2-1/libisofs/joliet.c/#L2223
    // ECMA-119 Annex B.2(f), its informative Joliet summary, lists zero
    // filler. Linux isofs selects Joliet by its escape sequence and does
    // not inspect these identifiers.
    if joliet {
        for field in block[702..813].chunks_exact_mut(37) {
            for character in field.chunks_exact_mut(2) {
                character.copy_from_slice(b"\0 ");
            }
        }
    } else {
        block[702..813].fill(b' ');
    }
    for offset in [813, 830, 864] {
        block[offset..offset + 17].copy_from_slice(b"2000010100000000\0");
    }
    block[847..864].copy_from_slice(b"0000000000000000\0");
    block[881] = 1;
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_layout_enforces_each_limit_including_padding() {
        assert!(layout([1, 1, MAX_GUEST, 1, 1, 1024, 16 * 1024]).is_some());
        assert!(layout([1, 1, MAX_GUEST + 1, 1, 1, 1, 1]).is_none());
        assert!(layout([1, 1, 1, 1, 1, 1025, 1]).is_none());
        assert!(layout([1, 1, 1, 1, 1, 1, 16 * 1024 + 1]).is_none());
        let exact = (MAX_SECTORS - JOLIET_ROOT - 7) as usize * SECTOR;
        assert_eq!(layout([1, 1, 1, exact, 1, 1, 1]).unwrap().1, MAX_SECTORS);
        assert!(layout([1, 1, 1, exact + 1, 1, 1, 1]).is_none());
    }

    #[test]
    fn windows_seed_layout_enforces_each_limit() {
        for (slot, cap) in [MAX_GUEST, 1024, 128, 32 * 1024, 16 * 1024]
            .into_iter()
            .enumerate()
        {
            let bytes = vec![1; if slot == 0 { 1 } else { cap + 1 }];
            let mut files: [&[u8]; 5] = [b"x"; 5];
            files[slot] = b"";
            assert!(Image::windows(files).is_none());
            if slot != 0 {
                files[slot] = &bytes;
                assert!(Image::windows(files).is_none());
                files[slot] = &bytes[..cap];
                assert!(Image::windows(files).is_some());
            }
        }
        // Check the 32 MiB guest limit without allocating a large fixture.
        assert!(file_extents([MAX_GUEST, 1, 1, 1, 1]).is_some());
    }
}
