// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::dir_block::DirBlock;
use crate::dir_entry::{DirEntry, DirEntryName};
use crate::error::Ext4Error;
use crate::extent::{Extent, Extents};
use crate::inode::{Inode, InodeFlags, InodeIndex};
use crate::path::PathBuf;
use crate::util::usize_from_u32;
use crate::Ext4;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};

/// Iterator over each [`DirEntry`] in a directory inode.
pub struct ReadDir<'a> {
    fs: &'a Ext4,

    /// Path of the directory. This is stored in an `Rc` so that it can
    /// be shared with each `DirEntry` without cloning the path data.
    ///
    /// Note that this path may be empty, e.g. if `read_dir` was called
    /// with an inode rather than a path.
    path: Rc<PathBuf>,

    /// Iterator over the directory's extents.
    extents: Extents<'a>,

    /// The current extent.
    extent: Option<Extent>,

    /// The index of the current block within the extent.
    block_index: u64,

    /// The current block's data.
    block: Vec<u8>,

    /// The current byte offset within the block data.
    offset_within_block: usize,

    /// Whether the iterator is done (calls to `Iterator::next` will
    /// return `None`).
    is_done: bool,

    /// Whether the directory has an htree for fast lookups. The
    /// iterator doesn't directly use the htree, but it affects which
    /// blocks have checksums.
    has_htree: bool,

    /// Initial checksum using values from the directory's inode. This
    /// serves as the seed for directory block checksums.
    checksum_base: Checksum,

    /// Inode of the directory. Just used for error reporting.
    inode: InodeIndex,
}

impl<'a> ReadDir<'a> {
    pub(crate) fn new(
        fs: &'a Ext4,
        inode: &Inode,
        path: PathBuf,
    ) -> Result<Self, Ext4Error> {
        let has_htree = inode.flags.contains(InodeFlags::DIRECTORY_HTREE);

        Ok(Self {
            fs,
            path: Rc::new(path),
            extents: Extents::new(fs, inode)?,
            extent: None,
            block_index: 0,
            block: vec![0; usize_from_u32(fs.superblock.block_size)],
            offset_within_block: 0,
            is_done: false,
            has_htree,
            checksum_base: inode.checksum_base.clone(),
            inode: inode.index,
        })
    }

    // Step to the next entry.
    //
    // This is factored out of `Iterator::next` for clarity and ease of
    // returning errors.
    //
    // When this returns `Ok(None)`, the outer loop in `Iterator::next`
    // will call it again until it reaches an actual value.
    fn next_impl(&mut self) -> Result<Option<DirEntry>, Ext4Error> {
        // Get the extent, or get the next one if not set.
        let extent = if let Some(extent) = &self.extent {
            extent
        } else {
            match self.extents.next() {
                Some(Ok(extent)) => {
                    self.extent = Some(extent);
                    self.block_index = 0;
                    self.offset_within_block = 0;

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
        if self.block_index == u64::from(extent.num_blocks) {
            self.extent = None;
            return Ok(None);
        }

        // If a block has been fully processed, move to the next block
        // on the next iteration.
        let block_size = self.fs.superblock.block_size;
        if self.offset_within_block >= usize_from_u32(block_size) {
            self.block_index += 1;
            self.offset_within_block = 0;
            return Ok(None);
        }

        // If at the start of a new block, read it and verify the checksum.
        if self.offset_within_block == 0 {
            DirBlock {
                fs: self.fs,
                dir_inode: self.inode,
                extent,
                block_within_extent: self.block_index,
                has_htree: self.has_htree,
                checksum_base: self.checksum_base.clone(),
            }
            .read(&mut self.block)?;
        }

        let (entry, entry_size) = DirEntry::from_bytes(
            &self.block[self.offset_within_block..],
            self.inode,
            self.path.clone(),
        )?;
        self.offset_within_block += entry_size;

        Ok(entry)
    }
}

impl<'a> Debug for ReadDir<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Only include the path field. This matches the Debug impl for
        // `std::fs::ReadDir`.
        write!(f, r#"ReadDir("{:?}")"#, self.path)
    }
}

impl<'a> Iterator for ReadDir<'a> {
    type Item = Result<DirEntry, Ext4Error>;

    fn next(&mut self) -> Option<Result<DirEntry, Ext4Error>> {
        // In pseudocode, here's what the iterator is doing:
        //
        // for extent in extents(inode) {
        //   for block in extent.blocks {
        //     verify_checksum(block);
        //     for dir_entry in block {
        //       yield dir_entry;
        //     }
        //   }
        // }

        loop {
            if self.is_done {
                return None;
            }

            match self.next_impl() {
                Ok(Some(entry)) => return Some(Ok(entry)),
                Ok(None) => {
                    // Continue.
                }
                Err(err) => {
                    self.is_done = true;
                    return Some(Err(err));
                }
            }
        }
    }
}

/// Search a directory inode for an entry with the given `name`. If
/// found, return the entry's inode, otherwise return a `NotFound`
/// error.
pub(crate) fn get_dir_entry_inode_by_name(
    fs: &Ext4,
    dir_inode: &Inode,
    name: DirEntryName<'_>,
) -> Result<Inode, Ext4Error> {
    assert!(dir_inode.metadata.is_dir());

    // TODO: add faster lookup by hash, if the inode has
    // InodeFlags::DIRECTORY_HTREE.

    // The entry's `path()` method is not called, so the value of the
    // base path does not matter.
    let path = PathBuf::empty();

    for entry in ReadDir::new(fs, dir_inode, path)? {
        let entry = entry?;
        if entry.file_name() == name {
            return Inode::read(fs, entry.inode);
        }
    }

    Err(Ext4Error::NotFound)
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;

    fn load_test_disk() -> Ext4 {
        let fs_path = std::path::Path::new("test_data/test_disk1.bin");
        Ext4::load_from_path(fs_path).unwrap()
    }

    #[test]
    fn test_read_dir() {
        let fs = load_test_disk();
        let root_inode = fs.read_root_inode().unwrap();
        let root_path = crate::PathBuf::new("/");

        // Use the iterator to get all DirEntries in the root directory.
        let entries: Vec<_> = ReadDir::new(&fs, &root_inode, root_path)
            .unwrap()
            .map(|e| e.unwrap())
            .collect();

        // Check for a few expected entries.
        assert!(entries.iter().any(|e| e.file_name() == "."));
        assert!(entries.iter().any(|e| e.file_name() == ".."));
        assert!(entries.iter().any(|e| e.file_name() == "empty_file"));
        assert!(entries.iter().any(|e| e.file_name() == "empty_dir"));

        // Check for something that does not exist.
        assert!(!entries.iter().any(|e| e.file_name() == "does_not_exist"));
    }

    #[test]
    fn test_get_dir_entry_inode_by_name() {
        let fs = load_test_disk();
        let root_inode = fs.read_root_inode().unwrap();

        let lookup = |name| {
            get_dir_entry_inode_by_name(
                &fs,
                &root_inode,
                DirEntryName::try_from(name).unwrap(),
            )
        };

        // Check for a few expected entries.
        // '.' always links to self.
        assert_eq!(lookup(".").unwrap().index, root_inode.index);
        // '..' is normally parent, but in the root dir it's just the
        // root dir again.
        assert_eq!(lookup("..").unwrap().index, root_inode.index);
        // Don't check specific values of these since they might change
        // if the test disk is regenerated
        assert!(lookup("empty_file").is_ok());
        assert!(lookup("empty_dir").is_ok());

        // Check for something that does not exist.
        let err = lookup("does_not_exist").unwrap_err();
        assert!(matches!(err, Ext4Error::NotFound));
    }
}
