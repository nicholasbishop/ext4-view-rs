// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::inode::Inode;
use crate::util::{read_u32le, usize_from_u32};
use crate::{Ext4, Ext4Error};
use alloc::vec;
use alloc::vec::Vec;
use core::mem;

/// Block map iterator.
///
/// Block maps are how file data was stored prior to extents. Unlike
/// extents, each block of the file is stored. This makes block maps
/// much less storage efficient.
///
/// The root of the block map structure is stored directly in the
/// inode. It consists of 15 block indices. Here, as in the rest of the
/// block map structure, a block index is a `u32` containing the
/// absolute index of a block in the filesystem.
///
/// Indices `0..=11` in the root node are direct. That is, they point
/// directly to a block of file data.
///
/// Index 12 points to an indirect block. This block contains an array
/// of direct block indices. The size of the array depends on the block
/// size; a 1KiB block can store 256 indices (1024÷4).
///
/// Index 13 points to a doubly-indirect block. This block contains an
/// array of indirect block indices. Each of those indices points to a
/// block containing direct indices.
///
/// Index 14 points to a triply-indirect block. This block contains an
/// array of indirect block indices, each of which points to a block
/// that also contains indirect indices, each of which points to a block
/// containing direct indices.
///
/// Indices are only initialized up to the size of the file.
pub(super) struct BlockMap {
    fs: Ext4,

    /// Root of the block map. This is copied directly from the inode.
    level_0: [u32; 15],

    /// Index within `level_0`.
    level_0_index: usize,

    /// Number of blocks the iterator has yielded so far.
    num_blocks_yielded: u64,

    /// Total number of blocks in the file.
    num_blocks_total: u64,

    /// Iterators through the deeper levels of the block map.
    level_1: Option<IndirectBlockIter>,
    level_2: Option<DoubleIndirectBlockIter>,
    level_3: Option<TripleIndirectBlockIter>,

    is_done: bool,
}

impl BlockMap {
    const NUM_ENTRIES: usize = 15;

    pub(super) fn new(fs: Ext4, inode: &Inode) -> Self {
        let mut level_0 = [0; Self::NUM_ENTRIES];
        for (i, dst) in level_0.iter_mut().enumerate() {
            *dst = read_u32le(&inode.inline_data, i * mem::size_of::<u32>());
        }

        let num_blocks = inode
            .metadata
            .size_in_bytes
            .div_ceil(u64::from(fs.0.superblock.block_size));

        Self {
            fs,
            level_0,
            num_blocks_yielded: 0,
            num_blocks_total: num_blocks,
            level_0_index: 0,
            level_1: None,
            level_2: None,
            level_3: None,
            is_done: false,
        }
    }

    fn next_impl(&mut self) -> Result<Option<u64>, Ext4Error> {
        if self.num_blocks_yielded >= self.num_blocks_total {
            self.is_done = true;
            return Ok(None);
        }

        let Some(block_0) = self.level_0.get(self.level_0_index) else {
            self.is_done = true;
            return Ok(None);
        };

        let ret: u32 = if self.level_0_index <= 11 {
            self.level_0_index += 1;
            self.num_blocks_yielded += 1;
            *block_0
        } else if self.level_0_index == 12 {
            if let Some(level_1) = &mut self.level_1 {
                if let Some(block_index) = level_1.next() {
                    self.num_blocks_yielded += 1;
                    return Ok(Some(u64::from(block_index)));
                } else {
                    self.level_1 = None;
                    self.level_0_index += 1;
                    return Ok(None);
                }
            } else {
                self.level_1 =
                    Some(IndirectBlockIter::new(self.fs.clone(), *block_0)?);
                return Ok(None);
            }
        } else if self.level_0_index == 13 {
            if let Some(level_2) = &mut self.level_2 {
                if let Some(block_index) = level_2.next() {
                    let block_index = block_index?;
                    self.num_blocks_yielded += 1;
                    return Ok(Some(u64::from(block_index)));
                } else {
                    self.level_2 = None;
                    self.level_0_index += 1;
                    return Ok(None);
                }
            } else {
                self.level_2 = Some(DoubleIndirectBlockIter::new(
                    self.fs.clone(),
                    *block_0,
                )?);
                return Ok(None);
            }
        } else if self.level_0_index == 14 {
            if let Some(level_3) = &mut self.level_3 {
                if let Some(block_index) = level_3.next() {
                    let block_index = block_index?;
                    self.num_blocks_yielded += 1;
                    return Ok(Some(u64::from(block_index)));
                } else {
                    self.level_3 = None;
                    self.level_0_index += 1;
                    return Ok(None);
                }
            } else {
                self.level_3 = Some(TripleIndirectBlockIter::new(
                    self.fs.clone(),
                    *block_0,
                )?);
                return Ok(None);
            }
        } else {
            todo!();
        };

        Ok(Some(u64::from(ret)))
    }
}

impl_result_iter!(BlockMap, u64);

struct IndirectBlockIter {
    /// Indirect block data. The block contains an array of `u32`, each
    /// of which is a block number.
    block: Vec<u8>,

    /// Current index within the block. This is a byte index, so each
    /// iterator step moves it forward by four (the size of a `u32`).
    index_within_block: usize,
}

impl IndirectBlockIter {
    fn new(fs: Ext4, block_index: u32) -> Result<Self, Ext4Error> {
        let block_size = fs.0.superblock.block_size;
        assert_eq!(usize_from_u32(block_size) % mem::size_of::<u32>(), 0);

        let mut block = vec![0u8; usize_from_u32(block_size)];

        fs.read_bytes(
            u64::from(block_index) * u64::from(block_size),
            &mut block,
        )?;

        Ok(Self {
            block,
            index_within_block: 0,
        })
    }
}

impl Iterator for IndirectBlockIter {
    /// Absolute block index.
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.index_within_block >= self.block.len() {
            return None;
        }

        let block_index = read_u32le(&self.block, self.index_within_block);
        self.index_within_block += mem::size_of::<u32>();

        Some(block_index)
    }
}

struct DoubleIndirectBlockIter {
    fs: Ext4,
    indirect_0: IndirectBlockIter,
    indirect_1: Option<IndirectBlockIter>,
    is_done: bool,
}

impl DoubleIndirectBlockIter {
    fn new(fs: Ext4, block_index: u32) -> Result<Self, Ext4Error> {
        Ok(Self {
            indirect_0: IndirectBlockIter::new(fs.clone(), block_index)?,
            indirect_1: None,
            fs,
            is_done: false,
        })
    }

    fn next_impl(&mut self) -> Result<Option<u32>, Ext4Error> {
        if let Some(indirect_1) = &mut self.indirect_1 {
            if let Some(block_index) = indirect_1.next() {
                Ok(Some(block_index))
            } else {
                self.indirect_1 = None;
                Ok(None)
            }
        } else if let Some(block_index) = self.indirect_0.next() {
            self.indirect_1 =
                Some(IndirectBlockIter::new(self.fs.clone(), block_index)?);
            Ok(None)
        } else {
            self.is_done = true;
            Ok(None)
        }
    }
}

impl_result_iter!(DoubleIndirectBlockIter, u32);

struct TripleIndirectBlockIter {
    fs: Ext4,
    indirect_0: IndirectBlockIter,
    indirect_1: Option<DoubleIndirectBlockIter>,
    is_done: bool,
}

impl TripleIndirectBlockIter {
    fn new(fs: Ext4, block_index: u32) -> Result<Self, Ext4Error> {
        Ok(Self {
            indirect_0: IndirectBlockIter::new(fs.clone(), block_index)?,
            indirect_1: None,
            fs,
            is_done: false,
        })
    }

    fn next_impl(&mut self) -> Result<Option<u32>, Ext4Error> {
        if let Some(indirect_1) = &mut self.indirect_1 {
            if let Some(block_index) = indirect_1.next() {
                let block_index = block_index?;
                Ok(Some(block_index))
            } else {
                self.indirect_1 = None;
                Ok(None)
            }
        } else if let Some(block_index) = self.indirect_0.next() {
            self.indirect_1 = Some(DoubleIndirectBlockIter::new(
                self.fs.clone(),
                block_index,
            )?);
            Ok(None)
        } else {
            self.is_done = true;
            Ok(None)
        }
    }
}

impl_result_iter!(TripleIndirectBlockIter, u32);
