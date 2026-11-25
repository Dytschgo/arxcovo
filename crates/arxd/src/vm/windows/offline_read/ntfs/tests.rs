use super::*;
use std::{
    io::Cursor,
    time::{Duration, Instant},
};

use super::super::{Budget, qcow::Image};

const CLUSTER: usize = 65_536;
const START: usize = 64 * 512;
const MFT: usize = START + 4 * 4096;
const HIVE: usize = START + 20 * 4096;
const REG_KEY: &str = "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE";

fn put_le16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_le32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_le64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn put_be16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_be32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn put_be64(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_be_bytes());
}

fn cell(hive: &mut [u8], at: usize, value: &[u8]) {
    let size = (value.len() + 4).next_multiple_of(8);
    let start = 4096 + at;
    put_le32(hive, start, (-(size as i32)) as u32);
    hive[start + 4..start + 4 + value.len()].copy_from_slice(value);
}

fn hive() -> Vec<u8> {
    let mut hive = vec![0; 8192];
    hive[..4].copy_from_slice(b"regf");
    for (at, value) in [
        (4, 1),
        (8, 1),
        (20, 1),
        (24, 5),
        (32, 1),
        (36, 32),
        (40, 4096),
        (44, 1),
    ] {
        put_le32(&mut hive, at, value);
    }
    let check = hive[..508]
        .chunks_exact(4)
        .fold(0, |a, w| a ^ le32(w, 0).unwrap());
    put_le32(&mut hive, 508, check);
    hive[4096..4100].copy_from_slice(b"hbin");
    put_le32(&mut hive, 4104, 4096);
    let names = [
        "ROOT",
        "Microsoft",
        "Windows",
        "CurrentVersion",
        "Setup",
        "State",
    ];
    for (index, name) in names.into_iter().enumerate() {
        let at = 32 + index * 128;
        let mut node = vec![0; 112];
        node[..2].copy_from_slice(b"nk");
        put_le16(&mut node, 2, 0x20);
        if index != 0 {
            put_le32(&mut node, 16, (at - 128) as u32);
        }
        put_le16(&mut node, 72, name.len() as u16);
        node[76..76 + name.len()].copy_from_slice(name.as_bytes());
        if index < 5 {
            put_le32(&mut node, 20, 1);
            let mut list = [0; 8];
            list[..2].copy_from_slice(b"li");
            put_le16(&mut list, 2, 1);
            put_le32(&mut list, 4, (at + 128) as u32);
            let off = 800 + index * 16;
            put_le32(&mut node, 28, off as u32);
            cell(&mut hive, off, &list);
        } else {
            put_le32(&mut node, 36, 1);
            put_le32(&mut node, 40, 880);
        }
        cell(&mut hive, at, &node);
        put_le32(&mut hive, 4096 + at + 120, 8);
    }
    cell(&mut hive, 880, &888u32.to_le_bytes());
    let mut value = [0; 32];
    value[..2].copy_from_slice(b"vk");
    put_le16(&mut value, 2, 10);
    put_le32(&mut value, 4, (REG_KEY.len() as u32 + 1) * 2);
    put_le32(&mut value, 8, 928);
    put_le32(&mut value, 12, 1);
    put_le16(&mut value, 16, 1);
    value[20..30].copy_from_slice(b"ImageState");
    cell(&mut hive, 888, &value);
    let text: Vec<_> = REG_KEY
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    cell(&mut hive, 928, &text);
    let end = 928 + (text.len() + 4).next_multiple_of(8);
    put_le32(&mut hive, 4096 + end, (4096 - end) as u32);
    hive
}

fn resident_named(kind: u32, attr_name: &[u8], value: &[u8]) -> Vec<u8> {
    let name_offset = 24usize;
    let value_offset = (name_offset + attr_name.len()).next_multiple_of(8);
    let len = (value_offset + value.len()).next_multiple_of(8);
    let mut attr = vec![0; len];
    put_le32(&mut attr, 0, kind);
    put_le32(&mut attr, 4, len as u32);
    attr[9] = (attr_name.len() / 2) as u8;
    put_le16(
        &mut attr,
        10,
        if attr_name.is_empty() {
            0
        } else {
            name_offset as u16
        },
    );
    put_le32(&mut attr, 16, value.len() as u32);
    put_le16(&mut attr, 20, value_offset as u16);
    if !attr_name.is_empty() {
        attr[name_offset..name_offset + attr_name.len()].copy_from_slice(attr_name);
    }
    attr[value_offset..value_offset + value.len()].copy_from_slice(value);
    attr
}

fn resident(kind: u32, value: &[u8]) -> Vec<u8> {
    resident_named(kind, &[], value)
}

fn extent_runs(kind: u32, attr_name: &[u8], runs: &[(u8, u8)]) -> Vec<u8> {
    let total_clusters: u64 = runs.iter().map(|&(_, c)| u64::from(c)).sum();
    let name_offset = 64usize;
    let runs_offset = (name_offset + attr_name.len()).next_multiple_of(8);
    let mut encoded = Vec::new();
    let mut prev_lcn = 0i64;
    for &(start, count) in runs {
        let delta = i64::from(start) - prev_lcn;
        prev_lcn = i64::from(start);
        encoded.push(0x11);
        encoded.push(count);
        encoded.push(delta as i8 as u8);
    }
    encoded.push(0);
    let len = (runs_offset + encoded.len()).next_multiple_of(8);
    let mut attr = vec![0; len];
    put_le32(&mut attr, 0, kind);
    put_le32(&mut attr, 4, len as u32);
    attr[8] = 1;
    attr[9] = (attr_name.len() / 2) as u8;
    put_le16(
        &mut attr,
        10,
        if attr_name.is_empty() {
            0
        } else {
            name_offset as u16
        },
    );
    put_le64(&mut attr, 24, total_clusters - 1);
    put_le16(&mut attr, 32, runs_offset as u16);
    for at in [40, 48, 56] {
        put_le64(&mut attr, at, total_clusters * 4096);
    }
    if !attr_name.is_empty() {
        attr[name_offset..name_offset + attr_name.len()].copy_from_slice(attr_name);
    }
    attr[runs_offset..runs_offset + encoded.len()].copy_from_slice(&encoded);
    attr
}

fn extent(start: u8, clusters: u8) -> Vec<u8> {
    extent_runs(DATA, &[], &[(start, clusters)])
}

fn filename_bytes(parent: u64, name: &str) -> Vec<u8> {
    let mut filename = vec![0; 66 + name.len() * 2];
    put_le64(&mut filename, 0, (1 << 48) | parent);
    filename[64] = name.len() as u8;
    filename[65] = 1;
    for (index, unit) in name.encode_utf16().enumerate() {
        put_le16(&mut filename, 66 + 2 * index, unit);
    }
    filename
}

fn name(parent: u64, name: &str) -> Vec<u8> {
    resident(FILE_NAME, &filename_bytes(parent, name))
}

fn index_root_i30(parent: u64, children: &[(u64, &str)]) -> Vec<u8> {
    let mut body = vec![0u8; 32];
    put_le32(&mut body, 0, FILE_NAME);
    put_le32(&mut body, 4, 1); // collation rule
    put_le32(&mut body, 8, 4096); // index block size
    body[12] = 1; // clusters per index block
    // Node header starts at offset 16.
    put_le32(&mut body, 16, 16); // entries start offset from node header
    for &(child_num, child_name) in children {
        let key = filename_bytes(parent, child_name);
        let entry_len = (16 + key.len()).next_multiple_of(8);
        let mut entry = vec![0u8; entry_len];
        put_le64(&mut entry, 0, (1 << 48) | child_num);
        put_le16(&mut entry, 8, entry_len as u16);
        put_le16(&mut entry, 10, key.len() as u16);
        entry[16..16 + key.len()].copy_from_slice(&key);
        body.extend_from_slice(&entry);
    }
    // Sentinel end entry.
    let mut end = vec![0u8; 16];
    put_le16(&mut end, 8, 16);
    put_le32(&mut end, 12, 0x02);
    body.extend_from_slice(&end);
    let used = (body.len() - 16) as u32;
    put_le32(&mut body, 20, used);
    put_le32(&mut body, 24, used);
    resident_named(INDEX_ROOT, &I30_NAME, &body)
}

fn volume_info_attr(flags: u16) -> Vec<u8> {
    let mut info = [0u8; 12];
    info[8] = 3;
    info[9] = 1;
    put_le16(&mut info, 10, flags);
    resident(VOLUME_INFORMATION, &info)
}

fn record_at(backing: &mut [u8], byte_offset: usize, folder: bool, attrs: &[Vec<u8>]) {
    let record = &mut backing[byte_offset..byte_offset + 1024];
    record[..4].copy_from_slice(b"FILE");
    put_le16(record, 4, 48);
    put_le16(record, 6, 3);
    put_le16(record, 16, 1);
    put_le16(record, 20, 56);
    put_le16(record, 22, if folder { 3 } else { 1 });
    put_le32(record, 28, 1024);
    put_le16(record, 48, 0x12ab);
    let mut at = 56;
    for attr in attrs {
        record[at..at + attr.len()].copy_from_slice(attr);
        at += attr.len();
    }
    put_le32(record, at, u32::MAX);
    put_le32(record, 24, (at + 4) as u32);
    for end in [512, 1024] {
        put_le16(record, end - 2, 0x12ab);
    }
}

fn record(backing: &mut [u8], number: usize, folder: bool, attrs: &[Vec<u8>]) {
    record_at(backing, MFT + number * 1024, folder, attrs);
}

fn raw_image() -> Vec<u8> {
    let mut raw = vec![0; 4 * CLUSTER];
    let last = raw.len() as u64 / 512 - 1;
    let backup_entries_lba = last - 1;
    let end_usable = last - 2;
    raw[450] = 0xee;
    put_le32(&mut raw, 454, 1);
    raw[510..512].copy_from_slice(&[0x55, 0xaa]);
    let mut entries_copy = [0u8; 128];
    entries_copy[..16].copy_from_slice(&BASIC_DATA);
    entries_copy[16] = 0x12;
    put_le64(&mut entries_copy, 32, 64);
    put_le64(&mut entries_copy, 40, end_usable);
    raw[1024..1152].copy_from_slice(&entries_copy);
    let backup_entries_at = (backup_entries_lba * 512) as usize;
    raw[backup_entries_at..backup_entries_at + 128].copy_from_slice(&entries_copy);
    let entries_sum = crc32(&entries_copy);
    let disk_guid = [0x33u8; 16];

    let gpt = &mut raw[512..1024];
    gpt[..8].copy_from_slice(b"EFI PART");
    put_le32(gpt, 8, 0x0001_0000);
    put_le32(gpt, 12, 92);
    for (at, value) in [(24, 1), (32, last), (40, 34), (48, end_usable), (72, 2)] {
        put_le64(gpt, at, value);
    }
    gpt[56..72].copy_from_slice(&disk_guid);
    put_le32(gpt, 80, 1);
    put_le32(gpt, 84, 128);
    put_le32(gpt, 88, entries_sum);
    let check = crc32(&gpt[..92]);
    put_le32(gpt, 16, check);

    let backup_at = (last * 512) as usize;
    let backup = &mut raw[backup_at..backup_at + 512];
    backup[..8].copy_from_slice(b"EFI PART");
    put_le32(backup, 8, 0x0001_0000);
    put_le32(backup, 12, 92);
    for (at, value) in [
        (24, last),
        (32, 1),
        (40, 34),
        (48, end_usable),
        (72, backup_entries_lba),
    ] {
        put_le64(backup, at, value);
    }
    backup[56..72].copy_from_slice(&disk_guid);
    put_le32(backup, 80, 1);
    put_le32(backup, 84, 128);
    put_le32(backup, 88, entries_sum);
    let backup_check = crc32(&backup[..92]);
    put_le32(backup, 16, backup_check);

    let boot = &mut raw[START..START + 512];
    boot[3..11].copy_from_slice(b"NTFS    ");
    boot[510..512].copy_from_slice(&[0x55, 0xaa]);
    put_le16(boot, 11, 512);
    boot[13] = 8;
    put_le64(boot, 40, end_usable - 64 + 1);
    put_le64(boot, 48, 4);
    boot[64] = (-10i8) as u8;

    // Allocate records 0..16 in $MFT $BITMAP.
    let mft_bm = [0xffu8; 4];
    record(
        &mut raw,
        0,
        false,
        &[extent(4, 4), resident(BITMAP, &mft_bm)],
    );
    record(&mut raw, 3, false, &[volume_info_attr(0)]);
    record(&mut raw, 5, true, &[index_root_i30(5, &[(6, "Windows")])]);
    record(
        &mut raw,
        6,
        true,
        &[
            name(5, "Windows"),
            index_root_i30(6, &[(7, "System32"), (10, "Setup")]),
        ],
    );
    record(
        &mut raw,
        7,
        true,
        &[name(6, "System32"), index_root_i30(7, &[(8, "config")])],
    );
    record(
        &mut raw,
        8,
        true,
        &[name(7, "config"), index_root_i30(8, &[(9, "SOFTWARE")])],
    );
    record(
        &mut raw,
        10,
        true,
        &[name(6, "Setup"), index_root_i30(10, &[(11, "State")])],
    );
    record(
        &mut raw,
        11,
        true,
        &[name(10, "State"), index_root_i30(11, &[(12, "State.ini")])],
    );
    record(&mut raw, 9, false, &[name(8, "SOFTWARE"), extent(20, 2)]);
    let ini = format!("[State]\r\nImageState={REG_KEY}\r\n");
    record(
        &mut raw,
        12,
        false,
        &[name(11, "State.ini"), resident(DATA, ini.as_bytes())],
    );
    raw[HIVE..HIVE + 8192].copy_from_slice(&hive());
    raw
}

fn qcow(raw: &[u8], backing: Option<&str>) -> Vec<u8> {
    let start = 5 * CLUSTER;
    let data_clusters = if backing.is_some() {
        0
    } else {
        raw.len() / CLUSTER
    };
    let mut disk = vec![0; start + data_clusters * CLUSTER];
    disk[..4].copy_from_slice(b"QFI\xfb");
    put_be32(&mut disk, 4, 3);
    put_be32(&mut disk, 20, 16);
    put_be64(&mut disk, 24, raw.len() as u64);
    put_be32(&mut disk, 36, 1);
    put_be64(&mut disk, 40, 3 * CLUSTER as u64);
    put_be64(&mut disk, 48, CLUSTER as u64);
    put_be32(&mut disk, 56, 1);
    put_be32(&mut disk, 96, 4);
    put_be32(&mut disk, 100, 104);
    put_be64(&mut disk, CLUSTER, 2 * CLUSTER as u64);
    put_be64(&mut disk, 3 * CLUSTER, 4 * CLUSTER as u64);
    // Populate 16-bit refcount=1 for all allocated metadata + data clusters.
    let total_host_clusters = 5 + data_clusters;
    for c in 0..total_host_clusters {
        put_be16(&mut disk, 2 * CLUSTER + c * 2, 1);
    }
    if let Some(path) = backing {
        put_be64(&mut disk, 8, 128);
        put_be32(&mut disk, 16, path.len() as u32);
        put_be32(&mut disk, 104, 0xe279_2aca);
        put_be32(&mut disk, 108, 5);
        disk[112..117].copy_from_slice(b"qcow2");
        disk[128..128 + path.len()].copy_from_slice(path.as_bytes());
    } else {
        for (index, data) in raw.chunks_exact(CLUSTER).enumerate() {
            let at = start + index * CLUSTER;
            put_be64(&mut disk, 4 * CLUSTER + index * 8, at as u64);
            disk[at..at + CLUSTER].copy_from_slice(data);
        }
    }
    disk
}

fn inspect_with(qcow: Vec<u8>, budget: Budget) -> Result<(bool, bool)> {
    setup_state(&mut Image::open(Cursor::new(qcow), None, budget)?)
}

fn inspect(qcow: Vec<u8>) -> Result<(bool, bool)> {
    inspect_with(qcow, Budget::new(Instant::now() + Duration::from_secs(5)))
}

#[test]
fn released_disk_proves_both_offline_states_independently() {
    let mut raw = raw_image();
    assert_eq!(inspect(qcow(&raw, None)), Ok((true, true)));
    let at = HIVE + 4096 + 928 + 4;
    raw[at] = b'X';
    assert_eq!(inspect(qcow(&raw, None)), Ok((false, true)));
    raw[at] = b'I';
    let at = MFT + 12 * 1024;
    let ini = raw[at..at + 1024]
        .windows(REG_KEY.len())
        .position(|bytes| bytes == REG_KEY.as_bytes())
        .unwrap();
    raw[at + ini] = b'X';
    assert_eq!(inspect(qcow(&raw, None)), Ok((true, false)));
}

#[test]
fn backing_follows_only_the_exact_unbacked_held_layer() {
    use crate::vm::paths::LocalPath;
    let raw = raw_image();
    let golden = LocalPath::parse(r"D:\fixture\golden.qcow2").unwrap();
    let wrong = LocalPath::parse(r"D:\fixture\different.qcow2").unwrap();
    let overlay = qcow(&raw, Some(golden.as_str()));
    let open = |backing, pin| {
        Image::open(
            Cursor::new(overlay.clone()),
            Some((Cursor::new(backing), pin)),
            Budget::new(Instant::now() + Duration::from_secs(5)),
        )
    };
    assert_eq!(
        setup_state(&mut open(qcow(&raw, None), &golden).unwrap()),
        Ok((true, true))
    );
    assert!(open(qcow(&raw, None), &wrong).is_err());
    assert!(open(qcow(&raw, Some(golden.as_str())), &golden).is_err());
    assert_eq!(inspect(overlay), Err(Invalid));
}

#[test]
fn malformed_offsets_tables_and_lengths_refuse_without_panicking() {
    let raw = raw_image();
    let valid = qcow(&raw, None);
    for length in [0, 1, CLUSTER - 1, CLUSTER, 5 * CLUSTER, valid.len() - 1] {
        let mut truncated = valid[..length].to_vec();
        if length == valid.len() - 1 {
            // Read the truncated mapped cluster by putting GPT there.
            put_be64(&mut truncated, 4 * CLUSTER, 8 * CLUSTER as u64);
        }
        assert_eq!(inspect(truncated), Err(Invalid));
    }
    for entry in [1 << 62, 1 << 56, 0x200, 4 * CLUSTER as u64, u64::MAX] {
        let mut bad = valid.clone();
        put_be64(&mut bad, 4 * CLUSTER, entry);
        assert_eq!(inspect(bad), Err(Invalid));
    }
    for (at, value) in [
        (512 + 80, u32::MAX),                 // oversized GPT directory
        (MFT + 56 + 40, u32::MAX),            // MFT allocation outside volume
        (MFT + 5 * 1024 + 510, u32::MAX),     // torn NTFS fixup
        (HIVE + 4096 + 800 + 8, 32),          // key refers to its parent
        (HIVE + 4096 + 928, i32::MIN as u32), // overflow cell size
        (HIVE + 4096 + 888 + 12, u32::MAX),   // value data offset out of bounds
    ] {
        let mut bad = raw.clone();
        put_le32(&mut bad, at, value);
        assert_eq!(inspect(qcow(&bad, None)), Err(Invalid), "offset {at}");
    }
}

#[test]
fn a_partial_host_cluster_refuses_even_a_read_inside_its_existing_prefix() {
    let raw = [1u8; CLUSTER];
    let mut file = qcow(&raw, None);
    file.truncate(file.len() - 1);
    let mut image = Image::open(
        Cursor::new(file),
        None,
        Budget::new(Instant::now() + Duration::from_secs(5)),
    )
    .unwrap();
    assert_eq!(fixed_at::<8>(&mut image, 0), Err(Invalid));
}

#[test]
fn backup_gpt_and_qcow_refcount_inconsistencies_refuse() {
    let raw = raw_image();
    // Corrupt backup GPT entries without changing primary GPT.
    let mut bad_backup = raw.clone();
    let backup_entries_at = (bad_backup.len() / 512 - 2) * 512;
    bad_backup[backup_entries_at + 16] ^= 0xff;
    assert_eq!(inspect(qcow(&bad_backup, None)), Err(Invalid));

    // Unallocated (refcount=0) or shared (refcount=2) data cluster refuses.
    for refcount in [0u16, 2u16] {
        let mut bad_rc = qcow(&raw, None);
        put_be16(&mut bad_rc, 2 * CLUSTER + 5 * 2, refcount);
        assert_eq!(inspect(bad_rc), Err(Invalid), "refcount {refcount}");
    }
}

#[test]
fn authoritative_namespace_bitmap_volume_and_extension_checks_refuse_inconsistencies() {
    // Stale FILE_NAME absent from parent $I30 index refuses.
    let mut missing_i30 = raw_image();
    record(
        &mut missing_i30,
        8,
        true,
        &[name(7, "config"), index_root_i30(8, &[])],
    );
    assert_eq!(inspect(qcow(&missing_i30, None)), Err(Invalid));

    // Ambiguous duplicate $I30 entries pointing to distinct records refuse.
    let mut dup_i30 = raw_image();
    record(
        &mut dup_i30,
        8,
        true,
        &[
            name(7, "config"),
            index_root_i30(8, &[(9, "SOFTWARE"), (13, "SOFTWARE")]),
        ],
    );
    assert_eq!(inspect(qcow(&dup_i30, None)), Err(Invalid));

    // Cleared $MFT $BITMAP allocation bit for a selected record refuses.
    let mut unallocated_mft = raw_image();
    let mut bm = [0xffu8; 4];
    bm[1] &= !(1 << 1); // clear record 9 (SOFTWARE)
    record(
        &mut unallocated_mft,
        0,
        false,
        &[extent(4, 4), resident(BITMAP, &bm)],
    );
    assert_eq!(inspect(qcow(&unallocated_mft, None)), Err(Invalid));

    // Dirty $Volume flag refuses.
    let mut dirty_vol = raw_image();
    record(&mut dirty_vol, 3, false, &[volume_info_attr(0x0001)]);
    assert_eq!(inspect(qcow(&dirty_vol, None)), Err(Invalid));

    // Reparse point on a selected directory refuses.
    let mut reparse_dir = raw_image();
    record(
        &mut reparse_dir,
        7,
        true,
        &[
            name(6, "System32"),
            index_root_i30(7, &[(8, "config")]),
            resident(REPARSE, &[0; 16]),
        ],
    );
    assert_eq!(inspect(qcow(&reparse_dir, None)), Err(Invalid));

    // Extension record targeting a selected path record refuses.
    let mut ext_rec = raw_image();
    record(&mut ext_rec, 14, false, &[]);
    put_le64(&mut ext_rec, MFT + 14 * 1024 + 32, (1 << 48) | 9);
    assert_eq!(inspect(qcow(&ext_rec, None)), Err(Invalid));
}

#[test]
fn fragmented_mft_reads_and_budget_limits_refuse_exhaustion() {
    // Fragment $MFT across two non-contiguous runs: clusters 4..6 (records 0..8)
    // and clusters 12..14 (records 8..16).
    let mut frag = raw_image();
    let second_half = frag[MFT + 8 * 1024..MFT + 16 * 1024].to_vec();
    frag[MFT + 8 * 1024..MFT + 16 * 1024].fill(0);
    frag[START + 12 * 4096..START + 14 * 4096].copy_from_slice(&second_half);
    let mft_bm = [0xffu8; 4];
    record(
        &mut frag,
        0,
        false,
        &[
            extent_runs(DATA, &[], &[(4, 2), (12, 2)]),
            resident(BITMAP, &mft_bm),
        ],
    );
    assert_eq!(inspect(qcow(&frag, None)), Ok((true, true)));

    let valid = qcow(&raw_image(), None);
    // Expired deadline refuses.
    assert_eq!(
        inspect_with(
            valid.clone(),
            Budget::new(Instant::now() - Duration::from_secs(1))
        ),
        Err(Invalid)
    );
    // Exhausted I/O byte budget refuses.
    assert_eq!(
        inspect_with(
            valid.clone(),
            Budget::with_limits(
                CLUSTER as u64 * 2,
                2_000_000,
                4_000_000,
                256 * (1 << 20),
                Instant::now() + Duration::from_secs(5),
            )
        ),
        Err(Invalid)
    );
    // Exhausted step budget (inner hive/index/MFT loop) refuses.
    assert_eq!(
        inspect_with(
            valid.clone(),
            Budget::with_limits(
                1024 * (1 << 20),
                2_000_000,
                16,
                256 * (1 << 20),
                Instant::now() + Duration::from_secs(5),
            )
        ),
        Err(Invalid)
    );
    // Exhausted heap budget refuses oversized allocations.
    assert_eq!(
        inspect_with(
            valid,
            Budget::with_limits(
                1024 * (1 << 20),
                2_000_000,
                4_000_000,
                CLUSTER as u64,
                Instant::now() + Duration::from_secs(5),
            )
        ),
        Err(Invalid)
    );
}
