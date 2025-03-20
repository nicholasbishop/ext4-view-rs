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
use crate::util::usize_from_u32;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;

#[derive(Default)]
struct CacheEntry {
    block_index: FsBlockIndex,
    data: Box<[u8]>,
}

pub(crate) struct BlockCache {
    max_entries: usize,
    entries: VecDeque<CacheEntry>,

    max_blocks_per_read: u32,
    read_buf: Box<[u8]>,

    block_size: BlockSize,
}

impl BlockCache {
    pub(crate) fn new(
        max_entries: usize,
        block_size: BlockSize,
        max_blocks_per_read: u32,
    ) -> Self {
        // TODO: unwrap
        let read_buf_len = usize_from_u32(max_blocks_per_read)
            .checked_mul(block_size.to_usize())
            .unwrap();
        Self {
            max_entries,
            entries: VecDeque::new(),
            max_blocks_per_read,
            read_buf: vec![0; read_buf_len].into_boxed_slice(),
            block_size,
        }
    }

    pub(crate) fn max_blocks_per_read(&self) -> u32 {
        self.max_blocks_per_read
    }

    pub(crate) fn has_entry(&self, block_index: FsBlockIndex) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.block_index == block_index)
    }

    pub(crate) fn get_entry(
        &mut self,
        block_index: FsBlockIndex,
    ) -> Option<&[u8]> {
        // TODO: move to list head.

        let index = self
            .entries
            .iter()
            .position(|entry| entry.block_index == block_index)?;

        let entry = self.entries.remove(index).unwrap();
        self.entries.push_front(entry);

        Some(&*self.entries[0].data)
    }

    pub(crate) fn insert_blocks<F>(
        &mut self,
        block_index: FsBlockIndex,
        num_blocks: u32,
        f: F,
    ) -> Result<(), Ext4Error>
    where
        F: Fn(&mut [u8]) -> Result<(), Ext4Error>,
    {
        // TODO: precondition
        assert_ne!(num_blocks, 0);

        assert!(num_blocks <= self.max_blocks_per_read);

        // Read block(s) into the buffer.
        f(&mut self.read_buf)?;

        // Add blocks to the cache.
        for i in 0..num_blocks {
            // TODO: unwrap
            let block_index = block_index.checked_add(u64::from(i)).unwrap();

            // TODO: unwraps
            let start = usize_from_u32(i)
                .checked_mul(self.block_size.to_usize())
                .unwrap();
            let end = start.checked_add(self.block_size.to_usize()).unwrap();

            self.entries.push_front(CacheEntry {
                block_index,
                data: self.read_buf[start..end].into(),
            });
        }

        // Remove blocks from the cache if needed. TODO: this should
        // happen before adding. TODO: don't waste the allocations.

        while self.entries.len() > self.max_entries {
            self.entries.pop_back();
        }

        Ok(())
    }
}
