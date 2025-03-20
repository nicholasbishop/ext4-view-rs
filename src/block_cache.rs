// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::block_index::FsBlockIndex;
use crate::block_size::BlockSize;
use crate::error::Ext4Error;
use alloc::boxed::Box;
use alloc::vec;

#[derive(Clone, Default)]
struct CacheEntry {
    block_index: FsBlockIndex,
    offset_in_block_data: usize,
}

impl CacheEntry {
    fn is_in_use(&self) -> bool {
        self.block_index != 0
    }
}

pub(crate) struct BlockCache {
    entries: Box<[CacheEntry]>,
    block_data: Box<[u8]>,
    block_size: BlockSize,
}

impl BlockCache {
    pub(crate) fn new(num_entries: usize, block_size: BlockSize) -> Self {
        // TODO: unwrap
        let block_data_size =
            num_entries.checked_mul(block_size.to_usize()).unwrap();
        Self {
            entries: vec![CacheEntry::default(); num_entries]
                .into_boxed_slice(),
            block_data: vec![0; block_data_size].into_boxed_slice(),
            block_size,
        }
    }

    pub(crate) fn get_entry(&self, block_index: FsBlockIndex) -> Option<&[u8]> {
        for entry in &self.entries {
            if entry.block_index == block_index {
                // TODO: unwrap OK?
                let end = entry
                    .offset_in_block_data
                    .checked_add(self.block_size.to_usize())
                    .unwrap();
                return Some(&self.block_data[entry.offset_in_block_data..end]);
            }
        }

        None
    }

    pub(crate) fn insert_blocks<F>(
        &mut self,
        block_index: FsBlockIndex,
        num_blocks: usize,
        f: F,
    ) -> Result<(), Ext4Error>
    where
        F: Fn(&mut [u8]) -> Result<(), Ext4Error>,
    {
        // TODO: precondition
        assert_ne!(num_blocks, 0);
        assert!(num_blocks < self.entries.len());

        // How many entries need to be freed?
        let mut num_to_free = num_blocks;
        for entry in &self.entries {
            if !entry.is_in_use() {
                num_to_free -= 1;
                if num_to_free == 0 {
                    break;
                }
            }
        }

        for entry in self.entries.iter_mut().rev() {
            if num_to_free == 0 {
                break;
            }
            if entry.is_in_use() {
                
            }
        }

        // Ensure that there are `num_blocks` of contiguous free space
        // in `block_data`.
        

        todo!()
    }
}
