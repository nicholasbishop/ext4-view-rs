// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::block_index::FsBlockIndex;
use crate::extent::Extent;
use crate::inode::Inode;
use crate::iters::extents::Extents;
use crate::{Ext4, Ext4Error};

/// Iterator over blocks in a file that uses extents.
///
/// The iterator produces absolute block indices. Note that no blocks
/// are produced for holes in the file (i.e. parts of the file not
/// covered by an extent).
pub(super) struct ExtentsBlocks {
    /// Extent iterator.
    extents: Extents,

    /// Current extent, or `None` if a new extent needs to be fetched
    /// from the iterator.
    extent: Option<Extent>,

    /// Current block index within the extent.
    block_within_extent: u16,

    /// Whether the iterator is done (all calls to `next` will return `None`).
    is_done: bool,
}

impl ExtentsBlocks {
    pub(super) fn new(fs: Ext4, inode: &Inode) -> Result<Self, Ext4Error> {
        Ok(Self {
            extents: Extents::new(fs, inode)?,
            extent: None,
            block_within_extent: 0,
            is_done: false,
        })
    }

    fn next_impl(&mut self) -> Result<Option<FsBlockIndex>, Ext4Error> {
        // Get the extent, or get the next one if not set.
        let extent = if let Some(extent) = &self.extent {
            extent
        } else {
            match self.extents.next() {
                Some(Ok(extent)) => {
                    self.extent = Some(extent);
                    self.block_within_extent = 0;

                    // OK to unwrap since we just set it.
                    self.extent.as_ref().unwrap()
                }
                Some(Err(err)) => return Err(err),
                None => {
                    self.is_done = true;
                    return Ok(None);
                }
            }
        };

        // If all blocks in the extent have been processed, move to the
        // next extent on the next iteration.
        if self.block_within_extent == extent.num_blocks {
            self.extent = None;
            return Ok(None);
        }

        let block = extent.start_block + u64::from(self.block_within_extent);

        self.block_within_extent += 1;

        Ok(Some(block))
    }
}

// In pseudocode, here's what the iterator is doing:
//
// for extent in extents(inode) {
//   for block in extent.blocks {
//     yield block;
//   }
// }
impl_result_iter!(ExtentsBlocks, FsBlockIndex);
