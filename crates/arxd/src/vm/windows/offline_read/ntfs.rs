use std::collections::BTreeSet;

use super::{
    Invalid, Result, bytes, le16, le32, le64,
    qcow::{Disk, fixed_at},
    range,
};

const SECTOR: u64 = 512;
const BASIC_DATA: [u8; 16] = [
    0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99, 0xc7,
];
const MAX_RECORDS: u64 = 524_288;
const MAX_RUNS: usize = 16_384;
const MAX_INDEX_BLOCKS: usize = 4_096;
const MAX_MFT_BITMAP: u64 = MAX_RECORDS / 8 + 64;
const MAX_DIR_BITMAP: u64 = 64 * 1024;
const REF_MASK: u64 = (1 << 48) - 1;
const VOLUME_RECORD: u64 = 3;
const ROOT: u64 = 5;
const RECORD_BATCH: u64 = 65_536;
const LIST: u32 = 0x20;
const FILE_NAME: u32 = 0x30;
const VOLUME_INFORMATION: u32 = 0x70;
const DATA: u32 = 0x80;
const INDEX_ROOT: u32 = 0x90;
const INDEX_ALLOCATION: u32 = 0xa0;
const BITMAP: u32 = 0xb0;
const REPARSE: u32 = 0xc0;
const I30_NAME: [u8; 8] = [b'$', 0, b'I', 0, b'3', 0, b'0', 0];

#[derive(Clone, Copy, Eq, PartialEq)]
struct Region {
    start: u64,
    len: u64,
}

fn partition(disk: &mut impl Disk) -> Result<Region> {
    if !disk.len().is_multiple_of(SECTOR) || disk.len() < 34 * SECTOR {
        return Err(Invalid);
    }
    let last = disk.len() / SECTOR - 1;
    let mbr = fixed_at::<512>(disk, 0)?;
    let mut head = fixed_at::<512>(disk, SECTOR)?;
    let entries_lba = le64(&head, 72)?;
    let count = le32(&head, 80)? as usize;
    let stride = le32(&head, 84)? as usize;
    let size = le32(&head, 12)? as usize;
    let first = le64(&head, 40)?;
    let end = le64(&head, 48)?;
    let entries_crc = le32(&head, 88)?;
    let entries_sectors = (count as u64 * stride as u64).div_ceil(SECTOR);
    if mbr[510..] != [0x55, 0xaa]
        || mbr[450] != 0xee
        || le32(&mbr, 454)? != 1
        || &head[..8] != b"EFI PART"
        || le32(&head, 8)? != 0x0001_0000
        || !(92..=512).contains(&size)
        || le32(&head, 20)? != 0
        || le64(&head, 24)? != 1
        || le64(&head, 32)? != last
        || entries_lba != 2
        || !(1..=128).contains(&count)
        || stride != 128
        || first < entries_lba + entries_sectors
        || first > end
        || end >= last
    {
        return Err(Invalid);
    }
    let stored = le32(&head, 16)?;
    let disk_guid = head[56..72].to_vec();
    head[16..20].fill(0);
    if crc32(&head[..size]) != stored {
        return Err(Invalid);
    }
    // Validate the backup GPT header at the final LBA and require identical
    // partition entries, refusing torn or inconsistent GPT copies.
    let mut backup = fixed_at::<512>(disk, last * SECTOR)?;
    let backup_size = le32(&backup, 12)? as usize;
    let backup_entries_lba = le64(&backup, 72)?;
    if &backup[..8] != b"EFI PART"
        || le32(&backup, 8)? != 0x0001_0000
        || backup_size != size
        || le32(&backup, 20)? != 0
        || le64(&backup, 24)? != last
        || le64(&backup, 32)? != 1
        || le64(&backup, 40)? != first
        || le64(&backup, 48)? != end
        || backup[56..72] != disk_guid[..]
        || le32(&backup, 80)? as usize != count
        || le32(&backup, 84)? as usize != stride
        || le32(&backup, 88)? != entries_crc
        || backup_entries_lba <= end
        || backup_entries_lba
            .checked_add(entries_sectors)
            .is_none_or(|tail| tail > last)
    {
        return Err(Invalid);
    }
    let backup_stored = le32(&backup, 16)?;
    backup[16..20].fill(0);
    if crc32(&backup[..backup_size]) != backup_stored {
        return Err(Invalid);
    }

    disk.budget_mut().reserve_heap(count * stride * 2)?;
    let mut entries = vec![0; count * stride];
    disk.read_at(entries_lba * SECTOR, &mut entries)?;
    if crc32(&entries) != entries_crc {
        return Err(Invalid);
    }
    let mut backup_entries = vec![0; count * stride];
    disk.read_at(backup_entries_lba * SECTOR, &mut backup_entries)?;
    if backup_entries != entries {
        return Err(Invalid);
    }
    let mut ranges = Vec::new();
    let mut chosen = None;
    for entry in entries.chunks_exact(stride) {
        disk.budget_mut().spend_step(1)?;
        if entry[..16] == [0; 16] {
            continue;
        }
        let low = le64(entry, 32)?;
        let high = le64(entry, 40)?;
        if low < first
            || high > end
            || low > high
            || ranges.iter().any(|&(a, b)| low <= b && a <= high)
        {
            return Err(Invalid);
        }
        ranges.push((low, high));
        if entry[..16] == BASIC_DATA
            && chosen
                .replace(Region {
                    start: low * SECTOR,
                    len: (high - low + 1) * SECTOR,
                })
                .is_some()
        {
            return Err(Invalid);
        }
    }
    chosen.ok_or(Invalid)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut state = u32::MAX;
    for &b in bytes {
        state ^= u32::from(b);
        for _ in 0..8 {
            state = (state >> 1) ^ if state & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !state
}

struct Volume {
    region: Region,
    cluster: u64,
    record: usize,
    mft_start: u64,
}

impl Volume {
    fn read(disk: &mut impl Disk) -> Result<Self> {
        let mut region = partition(disk)?;
        let boot = fixed_at::<512>(disk, region.start)?;
        let sectors = le64(&boot, 40)?;
        let clusters = u64::from(boot[13]);
        if &boot[3..11] != b"NTFS    "
            || boot[510..] != [0x55, 0xaa]
            || le16(&boot, 11)? != SECTOR as u16
            || !clusters.is_power_of_two()
            || clusters > 128
            || sectors == 0
            || sectors > region.len / SECTOR
        {
            return Err(Invalid);
        }
        region.len = sectors * SECTOR;
        let cluster = clusters * SECTOR;
        let record_size = boot[64] as i8;
        let record = if record_size > 0 {
            record_size as u64 * cluster
        } else {
            1u64.checked_shl(u32::from(record_size.unsigned_abs()))
                .ok_or(Invalid)?
        };
        if !matches!(record, 1024 | 2048 | 4096) {
            return Err(Invalid);
        }
        let mft_start = le64(&boot, 48)?.checked_mul(cluster).ok_or(Invalid)?;
        range(mft_start, record, region.len)?;
        Ok(Self {
            region,
            cluster,
            record: record as usize,
            mft_start,
        })
    }

    fn named_stream(
        &self,
        record: &[u8],
        wanted_kind: u32,
        wanted_name: &[u8],
        limit: u64,
    ) -> Result<Option<Data>> {
        let mut stream = None;
        attributes(record, |kind, item| {
            if kind == LIST || kind == REPARSE {
                return Err(Invalid);
            }
            if kind != wanted_kind {
                return Ok(());
            }
            let name_len = usize::from(item[9]) * 2;
            let name_off = usize::from(le16(item, 10)?);
            let actual_name = if name_len == 0 {
                &[][..]
            } else {
                bytes(item, name_off, name_len)?
            };
            if actual_name != wanted_name {
                return Ok(());
            }
            if le16(item, 12)? != 0 || stream.is_some() {
                return Err(Invalid);
            }
            stream = Some(match item[8] {
                0 => {
                    let size = le32(item, 16)? as usize;
                    let start = usize::from(le16(item, 20)?);
                    if start < 24 || size == 0 || size as u64 > limit {
                        return Err(Invalid);
                    }
                    Data::Resident(bytes(item, start, size)?.to_vec())
                }
                1 => Data::Extent(self.runs(item, limit)?),
                _ => return Err(Invalid),
            });
            Ok(())
        })?;
        Ok(stream)
    }

    fn data(&self, record: &[u8], limit: u64) -> Result<Data> {
        self.named_stream(record, DATA, &[], limit)?.ok_or(Invalid)
    }

    fn runs(&self, item: &[u8], limit: u64) -> Result<Runs> {
        let size = le64(item, 48)?;
        let allocated = le64(item, 40)?;
        let initialized = le64(item, 56)?;
        let mut at = usize::from(le16(item, 32)?);
        let last_vcn = le64(item, 24)?;
        if size == 0
            || size > limit
            || size > allocated
            || initialized != size
            || !allocated.is_multiple_of(self.cluster)
            || at < 64
            || le64(item, 16)? != 0
            || le16(item, 34)? != 0
        {
            return Err(Invalid);
        }
        let mut segments = Vec::new();
        let mut logical = 0u64;
        let mut lcn = 0u64;
        while let Some(&tag) = item.get(at) {
            at += 1;
            if tag == 0 {
                if logical != allocated || last_vcn.checked_add(1) != Some(allocated / self.cluster)
                {
                    return Err(Invalid);
                }
                return Ok(Runs { size, segments });
            }
            if segments.len() == MAX_RUNS {
                return Err(Invalid);
            }
            let n_len = usize::from(tag & 15);
            let n_off = usize::from(tag >> 4);
            // A missing delta marks a sparse run, which this reader refuses.
            if !(1..=8).contains(&n_len) || !(1..=8).contains(&n_off) {
                return Err(Invalid);
            }
            let n = little(bytes(item, at, n_len)?, false);
            at += n_len;
            let delta = little(bytes(item, at, n_off)?, true) as i64;
            at += n_off;
            let len = n
                .checked_mul(self.cluster)
                .filter(|&v| v > 0)
                .ok_or(Invalid)?;
            lcn = lcn.checked_add_signed(delta).ok_or(Invalid)?;
            let position = lcn.checked_mul(self.cluster).ok_or(Invalid)?;
            range(position, len, self.region.len)?;
            range(logical, len, allocated)?;
            segments.push(Segment {
                logical,
                physical: self.region.start + position,
                len,
            });
            logical += len;
        }
        Err(Invalid)
    }

    fn read_stream(&self, disk: &mut impl Disk, stream: Data) -> Result<Vec<u8>> {
        match stream {
            Data::Resident(bytes) => {
                disk.budget_mut().reserve_heap(bytes.len())?;
                Ok(bytes)
            }
            Data::Extent(runs) => {
                let len = usize::try_from(runs.size).map_err(|_| Invalid)?;
                disk.budget_mut().reserve_heap(len)?;
                let mut data = vec![0; len];
                runs.read(disk, 0, &mut data)?;
                Ok(data)
            }
        }
    }

    fn verify_clean_volume(
        &self,
        disk: &mut impl Disk,
        mft: &Runs,
        mft_bitmap: &[u8],
    ) -> Result<()> {
        if !bitmap_allocated(mft_bitmap, VOLUME_RECORD) {
            return Err(Invalid);
        }
        let mut raw = vec![0; self.record];
        mft.read(disk, VOLUME_RECORD * self.record as u64, &mut raw)?;
        let record = file_record(&mut raw)?.ok_or(Invalid)?;
        if le64(record, 32)? != 0 {
            return Err(Invalid);
        }
        let mut saw_info = false;
        attributes(record, |kind, item| {
            if kind == LIST || kind == REPARSE {
                return Err(Invalid);
            }
            if kind == VOLUME_INFORMATION {
                if item[8] != 0 || item[9] != 0 || saw_info {
                    return Err(Invalid);
                }
                let size = le32(item, 16)? as usize;
                let start = usize::from(le16(item, 20)?);
                if start < 24 || size < 12 {
                    return Err(Invalid);
                }
                let info = bytes(item, start, size)?;
                let flags = le16(info, 10)?;
                // VOLUME_IS_DIRTY (0x0001) refuses unclean volume state.
                if flags & 0x0001 != 0 {
                    return Err(Invalid);
                }
                saw_info = true;
            }
            Ok(())
        })?;
        saw_info.then_some(()).ok_or(Invalid)
    }

    fn resolve_child(
        &self,
        disk: &mut impl Disk,
        mft: &Runs,
        mft_bitmap: &[u8],
        parent_ref: u64,
        wanted: &str,
        expect_folder: bool,
    ) -> Result<u64> {
        let parent_num = parent_ref & REF_MASK;
        if !bitmap_allocated(mft_bitmap, parent_num) {
            return Err(Invalid);
        }
        let mut raw = vec![0; self.record];
        mft.read(disk, parent_num * self.record as u64, &mut raw)?;
        let parent_record = file_record(&mut raw)?.ok_or(Invalid)?;
        if reference(parent_record, parent_num)? != parent_ref
            || le16(parent_record, 22)? & 2 == 0
            || le64(parent_record, 32)? != 0
        {
            return Err(Invalid);
        }

        let root_data = self
            .named_stream(parent_record, INDEX_ROOT, &I30_NAME, self.record as u64)?
            .ok_or(Invalid)?;
        let Data::Resident(root_bytes) = root_data else {
            return Err(Invalid);
        };
        if root_bytes.len() < 32 {
            return Err(Invalid);
        }
        // $I30 indexes FILE_NAME (0x30) attributes.
        if le32(&root_bytes, 0)? != FILE_NAME {
            return Err(Invalid);
        }
        let block_size = le32(&root_bytes, 8)? as usize;
        if !matches!(
            block_size,
            1024 | 2048 | 4096 | 8192 | 16384 | 32768 | 65536
        ) {
            return Err(Invalid);
        }
        let mut found: Option<u64> = None;
        scan_index_entries(
            &root_bytes[16..],
            parent_ref,
            wanted,
            &mut found,
            disk.budget_mut(),
        )?;

        let alloc = self.named_stream(
            parent_record,
            INDEX_ALLOCATION,
            &I30_NAME,
            (MAX_INDEX_BLOCKS as u64) * (block_size as u64),
        )?;
        let dir_bitmap = self.named_stream(parent_record, BITMAP, &I30_NAME, MAX_DIR_BITMAP)?;
        match (alloc, dir_bitmap) {
            (Some(Data::Extent(runs)), Some(bm_stream)) => {
                if !runs.size.is_multiple_of(block_size as u64) {
                    return Err(Invalid);
                }
                let bm = self.read_stream(disk, bm_stream)?;
                let block_count = runs.size / block_size as u64;
                if block_count > MAX_INDEX_BLOCKS as u64 {
                    return Err(Invalid);
                }
                disk.budget_mut().reserve_heap(block_size)?;
                let mut block = vec![0u8; block_size];
                for index in 0..block_count {
                    disk.budget_mut().spend_step(1)?;
                    if !bitmap_allocated(&bm, index) {
                        continue;
                    }
                    runs.read(disk, index * block_size as u64, &mut block)?;
                    let node = fixup_record(&mut block, b"INDX")?;
                    // VCN stored at offset 16 must match block's VCN.
                    let expected_vcn = if block_size as u64 >= self.cluster {
                        index * (block_size as u64 / self.cluster)
                    } else {
                        index
                    };
                    if le64(node, 16)? != expected_vcn {
                        return Err(Invalid);
                    }
                    scan_index_entries(
                        bytes(node, 24, node.len().checked_sub(24).ok_or(Invalid)?)?,
                        parent_ref,
                        wanted,
                        &mut found,
                        disk.budget_mut(),
                    )?;
                }
            }
            (None, None) => {}
            _ => return Err(Invalid),
        }

        let child_ref = found.ok_or(Invalid)?;
        let child_num = child_ref & REF_MASK;
        if child_ref == parent_ref || !bitmap_allocated(mft_bitmap, child_num) {
            return Err(Invalid);
        }
        // Cross-check the authoritative child MFT record: must match reference,
        // type, base record, no reparse/attribute-list, and carry a matching FILE_NAME.
        let mut child_raw = vec![0; self.record];
        mft.read(disk, child_num * self.record as u64, &mut child_raw)?;
        let child_record = file_record(&mut child_raw)?.ok_or(Invalid)?;
        let is_folder = le16(child_record, 22)? & 2 != 0;
        if reference(child_record, child_num)? != child_ref
            || is_folder != expect_folder
            || le64(child_record, 32)? != 0
        {
            return Err(Invalid);
        }
        let mut backlink_confirmed = false;
        attributes(child_record, |kind, item| {
            if kind == LIST || kind == REPARSE {
                return Err(Invalid);
            }
            if kind == FILE_NAME {
                if item[8] != 0 || item[9] != 0 {
                    return Err(Invalid);
                }
                let size = le32(item, 16)? as usize;
                let start = usize::from(le16(item, 20)?);
                if start < 24 {
                    return Err(Invalid);
                }
                let value = bytes(item, start, size)?;
                if matches_filename(value, parent_ref, wanted)? {
                    backlink_confirmed = true;
                }
            }
            Ok(())
        })?;
        backlink_confirmed.then_some(child_ref).ok_or(Invalid)
    }

    fn verify_no_extensions(
        &self,
        disk: &mut impl Disk,
        mft: &Runs,
        protected_refs: &BTreeSet<u64>,
    ) -> Result<()> {
        disk.budget_mut().reserve_heap(RECORD_BATCH as usize)?;
        let mut blocks = vec![0; RECORD_BATCH as usize];
        let mut offset = 0;
        while offset < mft.size {
            let size = RECORD_BATCH.min(mft.size - offset) as usize;
            let block = &mut blocks[..size];
            mft.read(disk, offset, block)?;
            for record in block.chunks_exact_mut(self.record) {
                disk.budget_mut().spend_step(1)?;
                let Some(record) = file_record(record)? else {
                    continue;
                };
                let base_ref = le64(record, 32)?;
                if base_ref != 0 && protected_refs.contains(&base_ref) {
                    return Err(Invalid);
                }
            }
            offset += size as u64;
        }
        Ok(())
    }

    fn witnesses(&self, disk: &mut impl Disk) -> Result<(bool, bool)> {
        let mut record = vec![0; self.record];
        disk.read_at(self.region.start + self.mft_start, &mut record)?;
        let record = file_record(&mut record)?.ok_or(Invalid)?;
        if le64(record, 32)? != 0 {
            return Err(Invalid);
        }
        let Data::Extent(mft) = self.data(record, MAX_RECORDS * self.record as u64)? else {
            return Err(Invalid);
        };
        if mft.segments.first().map(|run| run.physical) != Some(self.region.start + self.mft_start)
            || !mft.size.is_multiple_of(self.record as u64)
        {
            return Err(Invalid);
        }
        let mft_bitmap_stream = self
            .named_stream(record, BITMAP, &[], MAX_MFT_BITMAP)?
            .ok_or(Invalid)?;
        let mft_bitmap = self.read_stream(disk, mft_bitmap_stream)?;
        self.verify_clean_volume(disk, &mft, &mft_bitmap)?;

        if !bitmap_allocated(&mft_bitmap, ROOT) {
            return Err(Invalid);
        }
        let mut root_raw = vec![0; self.record];
        mft.read(disk, ROOT * self.record as u64, &mut root_raw)?;
        let root_record = file_record(&mut root_raw)?.ok_or(Invalid)?;
        if le16(root_record, 22)? & 2 == 0 || le64(root_record, 32)? != 0 {
            return Err(Invalid);
        }
        let root = reference(root_record, ROOT)?;

        let windows = self.resolve_child(disk, &mft, &mft_bitmap, root, "Windows", true)?;
        let system32 = self.resolve_child(disk, &mft, &mft_bitmap, windows, "System32", true)?;
        let config = self.resolve_child(disk, &mft, &mft_bitmap, system32, "config", true)?;
        let software = self.resolve_child(disk, &mft, &mft_bitmap, config, "SOFTWARE", false)?;
        let setup = self.resolve_child(disk, &mft, &mft_bitmap, windows, "Setup", true)?;
        let state = self.resolve_child(disk, &mft, &mft_bitmap, setup, "State", true)?;
        let ini = self.resolve_child(disk, &mft, &mft_bitmap, state, "State.ini", false)?;

        let path_ids = [root, windows, system32, config, software, setup, state, ini];
        let protected: BTreeSet<u64> = path_ids.into_iter().collect();
        if protected.len() != path_ids.len() {
            return Err(Invalid);
        }
        self.verify_no_extensions(disk, &mft, &protected)?;

        let registry = self.read_data(disk, &mft, software, super::setup::HIVE_LIMIT)?;
        let registry_good = super::setup::registry(&registry, disk.budget_mut())?;
        drop(registry);
        let ini = self.read_data(disk, &mft, ini, super::setup::INI_LIMIT)?;
        let ini_good = super::setup::state_ini(&ini, disk.budget_mut())?;
        Ok((registry_good, ini_good))
    }

    fn read_data(&self, disk: &mut impl Disk, mft: &Runs, id: u64, limit: u64) -> Result<Vec<u8>> {
        let mut record = vec![0; self.record];
        let number = id & REF_MASK;
        mft.read(disk, number * self.record as u64, &mut record)?;
        let record = file_record(&mut record)?.ok_or(Invalid)?;
        if reference(record, number)? != id || le16(record, 22)? & 2 != 0 || le64(record, 32)? != 0
        {
            return Err(Invalid);
        }
        let stream = self.data(record, limit)?;
        self.read_stream(disk, stream)
    }
}

fn bitmap_allocated(bitmap: &[u8], index: u64) -> bool {
    let byte = usize::try_from(index / 8).ok();
    let bit = (index % 8) as u8;
    byte.and_then(|i| bitmap.get(i))
        .is_some_and(|&value| value & (1 << bit) != 0)
}

fn matches_filename(value: &[u8], expected_parent: u64, wanted: &str) -> Result<bool> {
    let parent = le64(value, 0)?;
    let count = *value.get(64).ok_or(Invalid)?;
    let namespace = *value.get(65).ok_or(Invalid)?;
    if namespace > 3 || count == 0 {
        return Err(Invalid);
    }
    let name = bytes(value, 66, usize::from(count) * 2)?;
    Ok(namespace != 2 && parent == expected_parent && same_name(name, wanted))
}

fn scan_index_entries(
    node_header_and_entries: &[u8],
    parent_ref: u64,
    wanted: &str,
    found: &mut Option<u64>,
    budget: &mut super::Budget,
) -> Result<()> {
    if node_header_and_entries.len() < 16 {
        return Err(Invalid);
    }
    let start = le32(node_header_and_entries, 0)? as usize;
    let used = le32(node_header_and_entries, 4)? as usize;
    let allocated = le32(node_header_and_entries, 8)? as usize;
    if start < 16 || start > used || used > allocated || used > node_header_and_entries.len() {
        return Err(Invalid);
    }
    let mut at = start;
    while at < used {
        budget.spend_step(1)?;
        let header = bytes(node_header_and_entries, at, 16)?;
        let file_ref = le64(header, 0)?;
        let entry_len = usize::from(le16(header, 8)?);
        let key_len = usize::from(le16(header, 10)?);
        let flags = le32(header, 12)?;
        if entry_len < 16 || !entry_len.is_multiple_of(8) || at + entry_len > used {
            return Err(Invalid);
        }
        // Bit 1 (0x02) marks the last (sentinel) entry in the node.
        if flags & 0x02 != 0 {
            return Ok(());
        }
        if key_len < 66 || 16 + key_len > entry_len {
            return Err(Invalid);
        }
        let key = bytes(node_header_and_entries, at + 16, key_len)?;
        if matches_filename(key, parent_ref, wanted)? {
            if file_ref == 0 || found.is_some_and(|prev| prev != file_ref) {
                return Err(Invalid);
            }
            *found = Some(file_ref);
        }
        at += entry_len;
    }
    Err(Invalid)
}

fn same_name(bytes: &[u8], expected: &str) -> bool {
    bytes.len() == 2 * expected.len()
        && bytes
            .chunks_exact(2)
            .zip(expected.bytes())
            .all(|(unit, ch)| unit[1] == 0 && unit[0].eq_ignore_ascii_case(&ch))
}

fn reference(record: &[u8], number: u64) -> Result<u64> {
    let sequence = le16(record, 16)?;
    if sequence == 0 {
        return Err(Invalid);
    }
    Ok((u64::from(sequence) << 48) | number)
}

fn fixup_record<'a>(record: &'a mut [u8], magic: &[u8; 4]) -> Result<&'a [u8]> {
    if bytes(record, 0, 4)? != magic {
        return Err(Invalid);
    }
    let usa = usize::from(le16(record, 4)?);
    let count = usize::from(le16(record, 6)?);
    if usa < 24 || count != record.len() / SECTOR as usize + 1 || usa + count * 2 > record.len() {
        return Err(Invalid);
    }
    let sequence = le16(record, usa)?.to_le_bytes();
    for i in 1..count {
        let end = i * SECTOR as usize;
        if record[end - 2..end] != sequence {
            return Err(Invalid);
        }
        let original = le16(record, usa + 2 * i)?.to_le_bytes();
        record[end - 2..end].copy_from_slice(&original);
    }
    Ok(record)
}

fn file_record(record: &mut [u8]) -> Result<Option<&[u8]>> {
    if record.iter().all(|&n| n == 0) {
        return Ok(None);
    }
    if bytes(record, 0, 4)? != b"FILE" {
        return Err(Invalid);
    }
    let flags = le16(record, 22)?;
    if flags & 1 == 0 {
        return Ok(None);
    }
    let usa = usize::from(le16(record, 4)?);
    let count = usize::from(le16(record, 6)?);
    let attrs = usize::from(le16(record, 20)?);
    let used = le32(record, 24)? as usize;
    if usa < 42
        || count != record.len() / SECTOR as usize + 1
        || usa + count * 2 > attrs
        || attrs >= used
        || used > record.len()
        || le32(record, 28)? as usize != record.len()
    {
        return Err(Invalid);
    }
    let _ = fixup_record(record, b"FILE")?;
    Ok(Some(&record[..used]))
}

fn attributes(record: &[u8], mut observe: impl FnMut(u32, &[u8]) -> Result<()>) -> Result<()> {
    let mut start = le16(record, 20)? as usize;
    loop {
        let kind = le32(record, start)?;
        if kind == u32::MAX {
            return Ok(());
        }
        let len = le32(bytes(record, start, 8)?, 4)? as usize;
        if len < 24 || !len.is_multiple_of(8) {
            return Err(Invalid);
        }
        observe(kind, bytes(record, start, len)?)?;
        start += len;
    }
}

enum Data {
    Resident(Vec<u8>),
    Extent(Runs),
}

struct Runs {
    size: u64,
    segments: Vec<Segment>,
}

struct Segment {
    logical: u64,
    physical: u64,
    len: u64,
}

impl Runs {
    fn read(&self, disk: &mut impl Disk, mut at: u64, mut out: &mut [u8]) -> Result<()> {
        range(at, out.len() as u64, self.size)?;
        while !out.is_empty() {
            let index = self
                .segments
                .partition_point(|run| run.logical <= at)
                .checked_sub(1)
                .ok_or(Invalid)?;
            let segment = &self.segments[index];
            let inside = at - segment.logical;
            let size = segment
                .len
                .checked_sub(inside)
                .filter(|&n| n > 0)
                .ok_or(Invalid)?
                .min(out.len() as u64) as usize;
            let (chunk, rest) = out.split_at_mut(size);
            disk.read_at(segment.physical + inside, chunk)?;
            at += size as u64;
            out = rest;
        }
        Ok(())
    }
}

fn little(bytes: &[u8], signed: bool) -> u64 {
    let fill = if signed && bytes.last().is_some_and(|n| n & 0x80 != 0) {
        0xff
    } else {
        0
    };
    let mut extended = [fill; 8];
    extended[..bytes.len()].copy_from_slice(bytes);
    u64::from_le_bytes(extended)
}

pub(super) fn setup_state(disk: &mut impl Disk) -> Result<(bool, bool)> {
    Volume::read(disk)?.witnesses(disk)
}

#[cfg(test)]
mod tests;
