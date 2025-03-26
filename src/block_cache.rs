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

#[derive(Clone)]
struct CacheEntry {
    block_index: FsBlockIndex,
    data: Box<[u8]>,
}

pub(crate) struct BlockCache {
    entries: VecDeque<CacheEntry>,

    max_blocks_per_read: u32,
    read_buf: Box<[u8]>,

    block_size: BlockSize,
}

impl BlockCache {
    pub(crate) fn new(
        num_entries: usize,
        block_size: BlockSize,
        max_blocks_per_read: u32,
    ) -> Self {
        assert!(usize_from_u32(max_blocks_per_read) <= num_entries);

        // TODO: unwrap
        let read_buf_len = usize_from_u32(max_blocks_per_read)
            .checked_mul(block_size.to_usize())
            .unwrap();
        let entries = vec![
            CacheEntry {
                block_index: 0,
                data: vec![0; block_size.to_usize()].into_boxed_slice(),
            };
            num_entries
        ];
        Self {
            entries: VecDeque::from(entries),
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

        let num_entries = self.entries.len();

        // Add blocks to the cache.
        for i in 0..num_blocks {
            let mut entry = self.entries.pop_back().unwrap();

            // TODO: unwrap
            let block_index = block_index.checked_add(u64::from(i)).unwrap();

            // TODO: unwraps
            let start = usize_from_u32(i)
                .checked_mul(self.block_size.to_usize())
                .unwrap();
            let end = start.checked_add(self.block_size.to_usize()).unwrap();

            entry.block_index = block_index;
            entry.data.copy_from_slice(&self.read_buf[start..end]);

            self.entries.push_front(entry);
        }

        assert_eq!(self.entries.len(), num_entries);

        Ok(())
    }
}
