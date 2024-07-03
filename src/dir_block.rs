// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::error::{Corrupt, Ext4Error};
use crate::extent::Extent;
use crate::inode::InodeIndex;
use crate::util::{read_u16le, read_u32le, usize_from_u32};
use crate::Ext4;

#[derive(Debug, Eq, PartialEq)]
enum DirBlockType {
    /// Root node of an htree.
    Root,

    /// Non-root internal node of an htree.
    Internal,

    /// Leaf node of an htree, also the format used for all blocks in a
    /// directory without an htree.
    Leaf,
}

/// Struct for reading and validating a directory block.
#[derive(Clone)]
pub(crate) struct DirBlock<'a> {
    pub(crate) fs: &'a Ext4,

    /// Extent to read from.
    pub(crate) extent: &'a Extent,

    /// Block offset within the extent.
    pub(crate) block_within_extent: u64,

    /// Directory inode index.
    pub(crate) dir_inode: InodeIndex,

    /// Whether the directory has an htree.
    pub(crate) has_htree: bool,

    /// Checksum base copied from the dir inode.
    pub(crate) checksum_base: Checksum,
}

impl<'a> DirBlock<'a> {
    /// Read the directory block's contents into `block`.
    ///
    /// If checksums are enabled for the filesystem, the directory
    /// block's checksum will be verified.
    pub(crate) fn read(&self, block: &mut [u8]) -> Result<(), Ext4Error> {
        let block_size = self.fs.superblock.block_size;
        assert_eq!(block.len(), usize_from_u32(block_size));

        let block_index = self.extent.start_block + self.block_within_extent;
        self.fs
            .read_bytes(block_index * u64::from(block_size), block)?;

        if !self.fs.has_metadata_checksums() {
            return Ok(());
        }

        let block_type = self.get_block_type(block);

        let expected_checksum = self.read_expected_checksum(block);
        let actual_checksum = if block_type == DirBlockType::Leaf {
            self.calc_leaf_checksum(block)
        } else {
            self.calc_internal_checksum(block, block_type)
        };

        if actual_checksum.finalize() == expected_checksum {
            Ok(())
        } else {
            Err(Ext4Error::Corrupt(Corrupt::DirBlockChecksum(
                self.dir_inode.get(),
            )))
        }
    }

    /// Get the stored checksum from the last four bytes of the block.
    fn read_expected_checksum(&self, block: &[u8]) -> u32 {
        let offset = block.len() - 4;
        read_u32le(block, offset)
    }

    /// Calculate the checksum of a leaf block.
    fn calc_leaf_checksum(&self, block: &[u8]) -> Checksum {
        let tail_entry_size = 12;
        let tail_entry_offset = block.len() - tail_entry_size;

        let mut checksum = self.checksum_base.clone();
        checksum.update(&block[..tail_entry_offset]);

        checksum
    }

    /// Calculate the checksum of a non-leaf block.
    fn calc_internal_checksum(
        &self,
        block: &[u8],
        block_type: DirBlockType,
    ) -> Checksum {
        let tail_entry_size = 8;
        let tail_entry_offset = block.len() - tail_entry_size;

        let limit_offset = if block_type == DirBlockType::Root {
            0x20
        } else {
            0x8
        };
        let count = read_u16le(block, limit_offset + 2);
        let num_bytes = limit_offset + (usize::from(count) * 8);

        let mut checksum = self.checksum_base.clone();
        checksum.update(&block[..num_bytes]);
        checksum.update_u32_le(read_u32le(block, tail_entry_offset));
        checksum.update_u32_le(0);

        checksum
    }

    fn get_block_type(&self, block: &[u8]) -> DirBlockType {
        // Non-htree directories use the same format as leaf nodes in an htree.
        if !self.has_htree {
            return DirBlockType::Leaf;
        }

        // The first block of an htree is the root node.
        if self.block_within_extent == 0 && self.extent.block_within_file == 0 {
            return DirBlockType::Root;
        }

        // Other internal nodes are identified by the first record
        // having a length equal to the whole block.
        let first_rec_len = u32::from(read_u16le(block, 4));
        if first_rec_len == self.fs.superblock.block_size {
            DirBlockType::Internal
        } else {
            DirBlockType::Leaf
        }
    }
}
