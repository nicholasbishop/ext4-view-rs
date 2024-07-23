// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::dir_block::DirBlock;
use crate::error::{Corrupt, Ext4Error};
use crate::extent::Extents;
use crate::inode::{Inode, InodeIndex};
use crate::util::{read_u16le, read_u32le};
use crate::Ext4;

type DirHash = u32;
type ChildBlock = u32;

// Internal node of an htree.
//
// This stores a reference to the raw bytes of entries in an internal
// node (including the root node) of an htree.
//
// Each entry is eight bytes long.
//
// The first entry is a header with three fields:
// * limit (u16): the number of entries that could be present (including
//   the header entry). In other words, space has been allocated for
//   this many entries.
// * count (u16): the actual number of entries (including the header
//   entry).
// * zero_block (u32): the child block index to use when looking up
//   hashes that compare less-than the first "normal" entry's hash key.
//
// The remaining entries each contain two fields:
// * hash: the minimum hash for this block. All directory entries in
//   this block (or children of this block) have a hash greater than or
//   equal to the hash key.
// * block (u32): the child block index.
//
// The entries after the header are sorted by hash, allowing for
// efficient hash lookup with a binary search.
//
// Note that all block indices mentioned above are relative to the file,
// not the file system. E.g. index zero is the file's first block, not
// the first block in the filesystem.
//
// Example of entries in an internal node:
// 0:  122, 15, 1      (limit, count, zero_block)
// 1:  0x0d69cdd8, 15  (hash, block)
// 2:  0x1eb8a274, 7   (hash, block)
// 3:  0x31df5aa2, 12  (hash, block)
// 4:  0x418c4380, 3   (hash, block)
// [...]
// 14: 0xec5cb0ca, 10  (hash, block)
struct InternalNode<'a> {
    /// Raw entry data. The header entry is included. Entries that are
    /// not in use are excluded (in other words, this includes entries
    /// up to `count`, not `limit`).
    entries: &'a [u8],
}

impl<'a> InternalNode<'a> {
    const ENTRY_SIZE: usize = 8;

    /// Create an `InternalNode` from a root directory block.
    fn from_root_block(
        block: &'a [u8],
        inode: InodeIndex,
    ) -> Result<Self, Ext4Error> {
        Self::new(&block[0x20..], inode)
    }

    /// Create an `InternalNode` from a non-root directory block.
    fn from_non_root_block(
        block: &'a [u8],
        inode: InodeIndex,
    ) -> Result<Self, Ext4Error> {
        Self::new(&block[0x8..], inode)
    }

    /// Create an `InternalNode` from raw bytes. These bytes come from a
    /// directory block, see [`from_root_block`] and [`from_non_root_block`].
    fn new(mut bytes: &'a [u8], inode: InodeIndex) -> Result<Self, Ext4Error> {
        let err = Ext4Error::Corrupt(Corrupt::DirEntry(inode.get()));

        // At least the header entry must be present.
        if bytes.len() < Self::ENTRY_SIZE {
            return Err(err);
        }

        // Get number of in-use entries from the header.
        let count = usize::from(read_u16le(bytes, 2));

        // Shrink raw data to exactly the valid length, or return an
        // error if not enough data.
        bytes = bytes.get(..Self::ENTRY_SIZE * count).ok_or(err)?;

        Ok(Self { entries: bytes })
    }

    /// Look up the entry at `index`. Returns `(hash, block)`.
    /// Panics if `index` is out of range.
    ///
    /// For `index` zero, the `hash` key is implicitly zero.
    fn get_entry(&self, index: usize) -> (DirHash, ChildBlock) {
        let offset = Self::ENTRY_SIZE * index;
        let block = read_u32le(self.entries, offset + 4);

        let hash = if index == 0 {
            0
        } else {
            read_u32le(self.entries, offset)
        };

        (hash, block)
    }

    /// Get the number of entries (this is based on the `count` field,
    /// not the `limit` field).
    fn num_entries(&self) -> usize {
        self.entries.len() / Self::ENTRY_SIZE
    }

    /// Perform a binary search to find the child block index for the
    /// `lookup_hash`.
    fn lookup_block_by_hash(&self, lookup_hash: DirHash) -> ChildBlock {
        // Left/right entry index.
        let mut left = 0;
        let mut right = self.num_entries() - 1;

        while left <= right {
            let mid = (left + right) / 2;
            let mid_hash = self.get_entry(mid).0;
            if mid_hash <= lookup_hash {
                left = mid + 1;
            } else {
                right = mid - 1;
            }
        }

        self.get_entry(left - 1).1
    }
}

/// Read the block containing the root node of an htree into
/// `block`. This is always the first block of the file.
fn read_root_block(
    fs: &Ext4,
    inode: &Inode,
    block: &mut [u8],
) -> Result<(), Ext4Error> {
    let mut extents = Extents::new(fs, inode)?;

    // Get the first extent.
    let extent = extents.next().ok_or_else(|| {
        Ext4Error::Corrupt(Corrupt::DirEntry(inode.index.get()))
    })??;

    // Read the first block of the extent.
    let dir_block = DirBlock {
        fs,
        dir_inode: inode.index,
        extent: &extent,
        block_within_extent: 0,
        has_htree: true,
        checksum_base: inode.checksum_base.clone(),
    };
    dir_block.read(block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_node() {
        let inode = InodeIndex::new(1).unwrap();

        let mut bytes = Vec::new();
        let add_entry =
            |bytes: &mut Vec<u8>, hash: DirHash, block: ChildBlock| {
                bytes.extend(hash.to_le_bytes());
                bytes.extend(block.to_le_bytes());
            };
        bytes.extend(20u16.to_le_bytes()); // limit
        bytes.extend(11u16.to_le_bytes()); // count
        bytes.extend(100u32.to_le_bytes()); // block

        add_entry(&mut bytes, 2, 199);
        add_entry(&mut bytes, 4, 198);
        add_entry(&mut bytes, 6, 197);
        add_entry(&mut bytes, 8, 196);
        add_entry(&mut bytes, 10, 195);

        add_entry(&mut bytes, 12, 194);
        add_entry(&mut bytes, 14, 193);
        add_entry(&mut bytes, 16, 192);
        add_entry(&mut bytes, 18, 191);
        add_entry(&mut bytes, 20, 190);

        // Test search with an odd number of entries.
        let node = InternalNode::new(&bytes, inode).unwrap();
        assert_eq!(node.num_entries(), 11);
        assert_eq!(node.get_entry(0), (0, 100));
        assert_eq!(node.get_entry(10), (20, 190));
        assert_eq!(node.lookup_block_by_hash(0), 100);
        assert_eq!(node.lookup_block_by_hash(9), 196);
        assert_eq!(node.lookup_block_by_hash(10), 195);
        assert_eq!(node.lookup_block_by_hash(11), 195);
        assert_eq!(node.lookup_block_by_hash(12), 194);
        assert_eq!(node.lookup_block_by_hash(20), 190);
        assert_eq!(node.lookup_block_by_hash(30), 190);

        // Add one more entry.
        bytes[2..4].copy_from_slice(&12u16.to_le_bytes()); // count
        add_entry(&mut bytes, 30, 189);

        // Test search with an even number of entries.
        let node = InternalNode::new(&bytes, inode).unwrap();
        assert_eq!(node.num_entries(), 12);
        assert_eq!(node.lookup_block_by_hash(0), 100);
        assert_eq!(node.lookup_block_by_hash(9), 196);
        assert_eq!(node.lookup_block_by_hash(10), 195);
        assert_eq!(node.lookup_block_by_hash(11), 195);
        assert_eq!(node.lookup_block_by_hash(12), 194);
        assert_eq!(node.lookup_block_by_hash(20), 190);
        assert_eq!(node.lookup_block_by_hash(30), 189);
    }
}
