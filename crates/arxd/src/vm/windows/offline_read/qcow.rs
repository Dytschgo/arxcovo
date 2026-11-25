use std::{
    collections::BTreeSet,
    io::{Read, Seek, SeekFrom},
};

use crate::vm::{paths::LocalPath, qemu_img::inspect_header};

use super::{Budget, Invalid, Result, be16, be32, be64, range};

const SHIFT: u32 = 16;
const CLUSTER: u64 = 1 << SHIFT;
const TABLE_ENTRIES: u64 = CLUSTER / 8;
const REFCOUNT_ENTRIES_PER_BLOCK: u64 = CLUSTER / 2;
const OFFSET: u64 = ((1 << 56) - 1) & !(CLUSTER - 1);
const COPIED: u64 = 1 << 63;
const MAX_DISK: u64 = 200 * (1 << 30);

pub(super) trait Disk {
    fn len(&self) -> u64;
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> Result<()>;
    fn budget_mut(&mut self) -> &mut Budget;
}

struct Layer<R> {
    input: R,
    physical: u64,
    virtual_size: u64,
    metadata: BTreeSet<u64>,
    l1: Vec<Option<u64>>,
    refcount_blocks: Vec<Option<u64>>,
}

impl<R: Read + Seek> Layer<R> {
    fn open(mut input: R, backing: Option<&LocalPath>, budget: &mut Budget) -> Result<Self> {
        let physical = input.seek(SeekFrom::End(0)).map_err(|_| Invalid)?;
        if !(CLUSTER..=MAX_DISK).contains(&physical) {
            return Err(Invalid);
        }
        budget.reserve_heap(CLUSTER as usize)?;
        let mut header = vec![0; CLUSTER as usize];
        read_file(&mut input, budget, 0, &mut header)?;
        let virtual_size = inspect_header(&header, backing).map_err(|_| Invalid)?;
        // Also refuse compression and require 16-bit refcounts (order 4).
        if virtual_size > MAX_DISK || be64(&header, 72)? != 0 || be32(&header, 96)? != 4 {
            return Err(Invalid);
        }
        let l1_count = u64::from(be32(&header, 36)?);
        let l1_offset = be64(&header, 40)?;
        let refcount_offset = be64(&header, 48)?;
        if l1_count != virtual_size.div_ceil(TABLE_ENTRIES * CLUSTER) || be32(&header, 56)? != 1 {
            return Err(Invalid);
        }

        let mut metadata = BTreeSet::from([0]);
        for at in [l1_offset, refcount_offset] {
            add_metadata(&mut metadata, at, physical)?;
        }
        let l1_byte_len = l1_count
            .checked_mul(8)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(Invalid)?;
        budget.reserve_heap(l1_byte_len)?;
        let mut l1_bytes = vec![0; l1_byte_len];
        read_file(&mut input, budget, l1_offset, &mut l1_bytes)?;
        let mut l1 = Vec::with_capacity(l1_count as usize);
        for chunk in l1_bytes.chunks_exact(8) {
            budget.spend_step(1)?;
            let pointer = be64(chunk, 0)?;
            if pointer == 0 {
                l1.push(None);
                continue;
            }
            let at = decode_offset(pointer, COPIED)?;
            add_metadata(&mut metadata, at, physical)?;
            l1.push(Some(at));
        }
        budget.reserve_heap(CLUSTER as usize)?;
        let mut refcounts = vec![0; CLUSTER as usize];
        read_file(&mut input, budget, refcount_offset, &mut refcounts)?;
        // A refcount block covers CLUSTER/2 host clusters for 16-bit entries.
        let used = physical.div_ceil(CLUSTER * REFCOUNT_ENTRIES_PER_BLOCK);
        let mut refcount_blocks = vec![None; usize::try_from(used).map_err(|_| Invalid)?];
        for (index, chunk) in refcounts.chunks_exact(8).enumerate() {
            budget.spend_step(1)?;
            let at = be64(chunk, 0)?;
            if at != 0 {
                if index as u64 >= used {
                    return Err(Invalid);
                }
                add_metadata(&mut metadata, at, physical)?;
                refcount_blocks[index] = Some(at);
            }
        }
        let mut layer = Self {
            input,
            physical,
            virtual_size,
            metadata,
            l1,
            refcount_blocks,
        };
        // Every referenced metadata cluster must be allocated with refcount == 1.
        let meta_clusters: Vec<u64> = layer.metadata.iter().copied().collect();
        for cluster_at in meta_clusters {
            layer.require_refcount_one(budget, cluster_at)?;
        }
        Ok(layer)
    }

    fn require_refcount_one(&mut self, budget: &mut Budget, cluster_at: u64) -> Result<()> {
        if cluster_at & (CLUSTER - 1) != 0 {
            return Err(Invalid);
        }
        range(cluster_at, CLUSTER, self.physical)?;
        let host_cluster = cluster_at >> SHIFT;
        let block_idx =
            usize::try_from(host_cluster / REFCOUNT_ENTRIES_PER_BLOCK).map_err(|_| Invalid)?;
        let entry_idx = host_cluster % REFCOUNT_ENTRIES_PER_BLOCK;
        let block_at = self
            .refcount_blocks
            .get(block_idx)
            .copied()
            .flatten()
            .ok_or(Invalid)?;
        let mut raw = [0u8; 2];
        read_file(&mut self.input, budget, block_at + entry_idx * 2, &mut raw)?;
        if be16(&raw, 0)? != 1 {
            return Err(Invalid);
        }
        Ok(())
    }

    /// Returns false only for an unallocated mapping, which falls through
    /// to the already held backing layer or to zeroes.
    fn read_cluster(&mut self, budget: &mut Budget, address: u64, out: &mut [u8]) -> Result<bool> {
        let cluster = address >> SHIFT;
        let within = address & (CLUSTER - 1);
        let index = usize::try_from(cluster / TABLE_ENTRIES).map_err(|_| Invalid)?;
        let Some(l2) = *self.l1.get(index).ok_or(Invalid)? else {
            return Ok(false);
        };
        let mut entry = [0; 8];
        let pointer_at = l2 + (cluster % TABLE_ENTRIES) * 8;
        read_file(&mut self.input, budget, pointer_at, &mut entry)?;
        let entry = be64(&entry, 0)?;
        if entry == 0 {
            return Ok(false);
        }
        let at = decode_offset(entry, COPIED | 1)?;
        if at != 0 {
            range(at, CLUSTER, self.physical)?;
            if self.metadata.contains(&at) {
                return Err(Invalid);
            }
            self.require_refcount_one(budget, at)?;
        }
        if entry & 1 != 0 {
            // An explicitly zeroed cluster does not consult the backing.
            if at == 0 && entry & COPIED != 0 {
                return Err(Invalid);
            }
            out.fill(0);
        } else {
            if at == 0 {
                return Err(Invalid);
            }
            read_file(&mut self.input, budget, at + within, out)?;
        }
        Ok(true)
    }
}

fn decode_offset(value: u64, flags: u64) -> Result<u64> {
    if value & !(OFFSET | flags) != 0 {
        return Err(Invalid);
    }
    Ok(value & OFFSET)
}

fn add_metadata(names: &mut BTreeSet<u64>, at: u64, physical: u64) -> Result<()> {
    range(at, CLUSTER, physical)?;
    if at & (CLUSTER - 1) != 0 || !names.insert(at) {
        return Err(Invalid);
    }
    Ok(())
}

fn read_file(
    reader: &mut (impl Read + Seek),
    budget: &mut Budget,
    at: u64,
    out: &mut [u8],
) -> Result<()> {
    budget.spend_io(out.len())?;
    reader.seek(SeekFrom::Start(at)).map_err(|_| Invalid)?;
    reader.read_exact(out).map_err(|_| Invalid)?;
    budget.check_deadline()
}

pub(super) struct Image<R> {
    top: Layer<R>,
    golden: Option<Layer<R>>,
    budget: Budget,
}

impl<R: Read + Seek> Image<R> {
    pub(super) fn open(
        top: R,
        golden: Option<(R, &LocalPath)>,
        mut budget: Budget,
    ) -> Result<Self> {
        let top = Layer::open(top, golden.as_ref().map(|(_, path)| *path), &mut budget)?;
        let golden = golden
            .map(|(file, _)| Layer::open(file, None, &mut budget))
            .transpose()?;
        if golden
            .as_ref()
            .is_some_and(|backing| backing.virtual_size != top.virtual_size)
        {
            return Err(Invalid);
        }
        Ok(Self {
            top,
            golden,
            budget,
        })
    }
}

impl<R: Read + Seek> Disk for Image<R> {
    fn len(&self) -> u64 {
        self.top.virtual_size
    }

    fn read_at(&mut self, mut at: u64, mut out: &mut [u8]) -> Result<()> {
        range(at, out.len() as u64, self.len())?;
        while !out.is_empty() {
            // Count sparse and explicitly zeroed clusters in the same budget
            // as file reads, even if no backing file operation is needed.
            let size = (CLUSTER - (at & (CLUSTER - 1))).min(out.len() as u64) as usize;
            self.budget.spend_io(size)?;
            let (head, rest) = out.split_at_mut(size);
            if !self.top.read_cluster(&mut self.budget, at, head)? {
                head.fill(0);
                if let Some(backing) = &mut self.golden {
                    backing.read_cluster(&mut self.budget, at, head)?;
                }
            }
            at += size as u64;
            out = rest;
        }
        Ok(())
    }

    fn budget_mut(&mut self) -> &mut Budget {
        &mut self.budget
    }
}

pub(super) fn fixed_at<const N: usize>(disk: &mut impl Disk, at: u64) -> Result<[u8; N]> {
    let mut result = [0; N];
    disk.read_at(at, &mut result)?;
    Ok(result)
}
