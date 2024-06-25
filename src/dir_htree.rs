// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::dir_entry::{DirEntry, DirEntryName};
use crate::dir_entry_hash::dir_hash_md4_half;
use crate::error::{Corrupt, Ext4Error, Incompatible};
use crate::extent::Extents;
use crate::inode::{Inode, InodeIndex};
use crate::path::PathBuf;
use crate::util::{read_u16le, read_u32le, usize_from_u32};
use crate::Ext4;
use alloc::rc::Rc;
use alloc::vec;

// TODO
type DirHash = u32;
type ChildBlock = u32;

// Internal node of an htree.
//
// This stores a reference to the raw bytes of entries in an internal
// node of an htree (including the root node).
//
// Each entry is eight bytes long.
//
// The first entry is a header with three fields:
// * limit (u16): the number of entries that could be present (not used
//   in this code).
// * count (u16): the actual number of entries (including the header
//   entry).
// * zero_block (u32): the block index to use if the lookup_hash is less
//   than the next entry's hash key.
//
// The remaining entries each contain two fields:
// * hash: the minimum hash for this block. All directory entries in
//   this block (or children of this block) have a hash >= the hash key.
// * block (u32): the child block index.
//
// The entries after the header are storted by hash, allowing for
// efficient hash lookup with a binary search.
//
// Note that all block indices mentioned above are relative the file,
// not the file system. E.g. index zero is the file's first block, not
// the first block in the filesystem.
//
// Example:
// 0:  122, 15, 1      (limit, count, zero_block)
// 1:  0x0d69cdd8, 15  (hash, block)
// 2:  0x1eb8a274, 7   (hash, block)
// 3:  0x31df5aa2, 12  (hash, block)
// 4:  0x418c4380, 3   (hash, block)
// [...]
// 14: 0xec5cb0ca, 10  (hash, block)
struct InternalNode<'a> {
    /// Raw entry data. The header entry is included.
    entries: &'a [u8],
}

impl<'a> InternalNode<'a> {
    const ENTRY_SIZE: usize = 8;

    fn new(mut bytes: &'a [u8], inode: InodeIndex) -> Result<Self, Ext4Error> {
        let err = Ext4Error::Corrupt(Corrupt::DirEntry(inode.get()));

        // At least the header entry must be present.
        if bytes.len() < Self::ENTRY_SIZE {
            return Err(err);
        }

        // Get number of entries from the header.
        let count = usize::from(read_u16le(bytes, 2));

        // Shrink raw data to exactly the valid length, or return an
        // error if not enough data.
        bytes = bytes.get(..Self::ENTRY_SIZE * count).ok_or(err)?;

        Ok(Self { entries: bytes })
    }

    /// Look up the entry at `index`. Returns `(hash, block)`.
    /// Panics if `index` is out of range.
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

pub(crate) fn get_dir_entry_via_htree(
    fs: &Ext4,
    inode: &Inode,
    name: DirEntryName<'_>,
) -> Result<DirEntry, Ext4Error> {
    // TODO: block checksum.

    let block_size = fs.superblock.block_size;
    let mut block = vec![0; usize_from_u32(block_size)];

    let mut extents = Extents::new(fs, inode)?;
    // TODO: unwrap
    let extent = extents.next().unwrap()?;

    fs.read_bytes(extent.start_block * u64::from(block_size), &mut block)?;

    // Handle '.' and '..'.
    // TODO: could just delegate these to the higher level?
    if name == "." || name == ".." {
        let corrupt =
            || Ext4Error::Corrupt(Corrupt::DirBlock(inode.index.get()));
        let offset = if name == "." { 0 } else { 12 };
        let (entry, _size) = DirEntry::from_bytes(
            &block[offset..],
            inode.index,
            Rc::new(PathBuf::empty()),
        )?;
        let entry = entry.ok_or_else(corrupt)?;
        if entry.file_name() == name {
            return Ok(entry);
        } else {
            return Err(corrupt());
        }
    }

    let hash_type = block[0x1c];
    let depth = block[0x1e];

    // TODO
    // assert_eq!(depth, 0);

    // Currently only the "half MD4" algorithm is supported by this library.
    if hash_type != 1 {
        return Err(Ext4Error::Incompatible(Incompatible::DirectoryHash(
            hash_type,
        )));
    }

    let (hash, _minor_hash) =
        dir_hash_md4_half(name, &fs.superblock.htree_hash_seed);

    let root_node = InternalNode::new(&block[0x20..], inode.index)?;
    let mut child_block_relative = root_node.lookup_block_by_hash(hash);

    for level in 0..=depth {
        // Lookup child block.
        let mut extents = Extents::new(fs, inode)?;
        // TODO: unwraps
        let extent = extents
            .find(|e| {
                let e = e.as_ref().unwrap();
                child_block_relative >= e.block_within_file
                    && child_block_relative
                        < e.block_within_file + u32::from(e.num_blocks)
            })
            .unwrap()
            .unwrap();
        let child_block_absolute = extent.start_block
            + u64::from(child_block_relative - extent.block_within_file);

        fs.read_bytes(
            child_block_absolute * u64::from(block_size),
            &mut block,
        )?;

        if level != depth {
            // TODO
            let inner_node = InternalNode::new(&block[0x8..], inode.index)?;
            child_block_relative = inner_node.lookup_block_by_hash(hash);
        }
    }

    // TODO: figure out collisions.

    // Search through the block for the right entry.
    // TODO
    let path = Rc::new(PathBuf::empty());
    let mut offset_within_block = 0;
    while offset_within_block < block.len() {
        let (dir_entry, entry_size) = DirEntry::from_bytes(
            &block[offset_within_block..],
            inode.index,
            path.clone(),
        )?;
        offset_within_block += entry_size;
        let Some(dir_entry) = dir_entry else {
            continue;
        };

        if dir_entry.file_name() == name {
            return Ok(dir_entry);
        }
    }

    Err(Ext4Error::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Path, ReadDir};

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

    /// Use ReadDir to iterate over all directory entries. Check that
    /// each entry can be looked up directly via the htree.
    ///
    /// Returns the number of entries.
    #[track_caller]
    fn compare_all_entries(fs: &Ext4, dir: Path<'_>) -> usize {
        let dir_inode = fs.path_to_inode(dir).unwrap();
        let iter = ReadDir::new(fs, &dir_inode, PathBuf::from(dir)).unwrap();
        let mut count = 0;
        for iter_entry in iter {
            let iter_entry = iter_entry.unwrap();
            let htree_entry =
                get_dir_entry_via_htree(fs, &dir_inode, iter_entry.file_name())
                    .unwrap();
            assert_eq!(htree_entry.file_name(), iter_entry.file_name());
            assert_eq!(htree_entry.inode, iter_entry.inode);
            count += 1;
        }
        count
    }

    #[test]
    fn test_get_dir_entry_via_htree() {
        let data = include_bytes!("../test_data/test_disk1.bin");
        let fs = Ext4::load(Box::new(data.to_vec())).unwrap();

        // Resolve paths in `/medium_dir` via htree.
        let medium_dir = Path::new("/medium_dir");
        assert_eq!(compare_all_entries(&fs, medium_dir), 1_002);

        // Resolve paths in `/big_dir` via htree.
        let big_dir = Path::new("/big_dir");
        assert_eq!(compare_all_entries(&fs, big_dir), 10_002);
    }
}
