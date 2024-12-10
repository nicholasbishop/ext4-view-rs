// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::extent::Extent;
use crate::inode::{Inode, InodeIndex};
use crate::iters::extents::Extents;
use crate::{Corrupt, Ext4, Ext4Error};

// enum State {
//     Initial,
//     Extent {
//         extent: Extent,
//         block_within_extent: u16,
//     },
// }

/// Iterator over blocks in a file that uses extents.
///
/// The iterator produces absolute block indices. A block index of zero
/// indicates a hole.
pub(super) struct ExtentsBlocks {
    /// Extent iterator.
    extents: Extents,

    /// Current extent, or `None` if a new extent needs to be fetched
    /// from the iterator.
    extent: Option<Extent>,

    // TODO
    blocks_remaining_in_hole: u32,
    //block_within_hole: Option<u64>,

    // TODO
    //next_extent: Option<Extent>,
    /// Current block index within the extent.
    block_within_extent: u16,

    /// TODO
    block_within_file: u32,

    /// Total number of blocks in the file.
    num_blocks_total: u32,

    /// Whether the iterator is done (all calls to `next` will return `None`).
    is_done: bool,

    /// Just used for errors.
    inode: InodeIndex,
}

impl ExtentsBlocks {
    pub(super) fn new(fs: Ext4, inode: &Inode) -> Result<Self, Ext4Error> {
        // TODO: unwrap
        let num_blocks_total = u32::try_from(
            inode
                .metadata
                .size_in_bytes
                .div_ceil(fs.0.superblock.block_size.to_u64()),
        )
        .unwrap();

        Ok(Self {
            extents: Extents::new(fs, inode)?,
            extent: None,
            blocks_remaining_in_hole: 0,
            block_within_file: 0,
            num_blocks_total,
            block_within_extent: 0,
            is_done: false,
            inode: inode.index,
        })
    }

    fn next_impl(&mut self) -> Result<Option<u64>, Ext4Error> {
        if self.block_within_file >= self.num_blocks_total {
            self.is_done = true;
            return Ok(None);
        }

        // If in a hole, return zero and decrement the number of
        // remaining hole blocks.
        if self.blocks_remaining_in_hole > 0 {
            self.blocks_remaining_in_hole -= 1;
            self.block_within_file += 1;
            return Ok(Some(0));
        }

        // Get the extent, or get the next one if not set.
        let extent = if let Some(extent) = &self.extent {
            extent
        } else {
            match self.extents.next() {
                Some(Ok(extent)) => {
                    if extent.block_within_file > self.block_within_file {
                        self.blocks_remaining_in_hole =
                            extent.block_within_file - self.block_within_file;
                    }

                    self.extent = Some(extent);
                    self.block_within_extent = 0;

                    if self.blocks_remaining_in_hole > 0 {
                        return Ok(None);
                    }

                    // OK to unwrap since we just set it.
                    self.extent.as_ref().unwrap()
                }
                Some(Err(err)) => return Err(err),
                None => {
                    // TODO
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

        let block = extent
            .start_block
            .checked_add(u64::from(self.block_within_extent))
            .ok_or(Corrupt::ExtentBlock(self.inode.get()))?;

        // OK to unwrap: `block_within_extent` is less than `num_blocks`
        // (checked above) so adding `1` cannot fail.
        self.block_within_extent =
            self.block_within_extent.checked_add(1).unwrap();

        // OK to unwrap: `block_within_file` is less than
        // `num_blocks_total` (checked at the beginning of this
        // function), so adding 1 cannot fail.
        self.block_within_file = self.block_within_file.checked_add(1).unwrap();

        Ok(Some(block))
    }
}

impl_result_iter!(ExtentsBlocks, u64);

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_test_disk1, FollowSymlinks, Path};

    #[test]
    fn test_extents_blocks_with_hole() {
        let fs = load_test_disk1();

        let inode = fs
            .path_to_inode(Path::new("/holes"), FollowSymlinks::All)
            .unwrap();
        let block_indices: Vec<_> = ExtentsBlocks::new(fs, &inode)
            .unwrap()
            .map(|b| b.unwrap())
            .collect();

        let mut is_hole = vec![];
        for i in 0..5 {
            is_hole.extend([false; 4]);
            if i != 4 {
                is_hole.extend([true; 8]);
            }
        }

        // TODO: add test with hole at end.

        assert_eq!(block_indices.len(), is_hole.len());

        for (block_index, is_hole) in block_indices.iter().zip(is_hole.iter()) {
            assert_eq!(*block_index == 0, *is_hole);
        }
    }
}
