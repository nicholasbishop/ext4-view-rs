// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::Ext4;
use crate::block_index::FsBlockIndex;
use crate::checksum::Checksum;
use crate::dir_block::DirBlock;
use crate::dir_entry::DirEntry;
use crate::error::{CorruptKind, Ext4Error};
use crate::inode::{Inode, InodeFlags, InodeIndex};
use crate::iters::file_blocks::FileBlocks;
use crate::path::PathBuf;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};

/// Iterator over each [`DirEntry`] in a directory inode.
pub struct ReadDir {
    fs: Ext4,

    /// Path of the directory. This is stored in an `Rc` so that it can
    /// be shared with each `DirEntry` without cloning the path data.
    ///
    /// Note that this path may be empty, e.g. if `read_dir` was called
    /// with an inode rather than a path.
    path: Rc<PathBuf>,

    /// Iterator over the blocks of the directory.
    file_blocks: FileBlocks,

    /// Current absolute block index, or `None` if the next block needs
    /// to be fetched.
    block_index: Option<FsBlockIndex>,

    /// Whether this is the first block in the file.
    is_first_block: bool,

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

impl ReadDir {
    pub(crate) fn new(
        fs: Ext4,
        inode: &Inode,
        path: PathBuf,
    ) -> Result<Self, Ext4Error> {
        let has_htree = inode.flags.contains(InodeFlags::DIRECTORY_HTREE);

        if inode.flags.contains(InodeFlags::DIRECTORY_ENCRYPTED) {
            return Err(Ext4Error::Encrypted);
        }

        Ok(Self {
            fs: fs.clone(),
            path: Rc::new(path),
            file_blocks: FileBlocks::new(fs.clone(), inode)?,
            block_index: None,
            is_first_block: true,
            block: vec![0; fs.0.superblock.block_size.to_usize()],
            offset_within_block: 0,
            is_done: false,
            has_htree,
            checksum_base: inode.checksum_base.clone(),
            inode: inode.index,
        })
    }

    fn next_impl(&mut self) -> Result<Option<DirEntry>, Ext4Error> {
        // Get the block index, or get the next one if not set.
        let block_index = if let Some(block_index) = self.block_index {
            block_index
        } else {
            match self.file_blocks.next() {
                Some(Ok(block_index)) => {
                    self.block_index = Some(block_index);
                    self.offset_within_block = 0;

                    block_index
                }
                Some(Err(err)) => return Err(err),
                None => {
                    self.is_done = true;
                    return Ok(None);
                }
            }
        };

        // If a block has been fully processed, move to the next block
        // on the next iteration.
        let block_size = self.fs.0.superblock.block_size;
        if self.offset_within_block >= block_size {
            self.is_first_block = false;
            self.block_index = None;
            return Ok(None);
        }

        // If at the start of a new block, read it and verify the checksum.
        if self.offset_within_block == 0 {
            DirBlock {
                fs: &self.fs,
                dir_inode: self.inode,
                block_index,
                is_first: self.is_first_block,
                has_htree: self.has_htree,
                checksum_base: self.checksum_base.clone(),
            }
            .read(&mut self.block)?;
        }

        let (entry, entry_size) = DirEntry::from_bytes(
            self.fs.clone(),
            &self.block[self.offset_within_block..],
            self.inode,
            self.path.clone(),
        )?;

        self.offset_within_block = self
            .offset_within_block
            .checked_add(entry_size)
            .ok_or(CorruptKind::DirEntry(self.inode))?;

        Ok(entry)
    }
}

impl Debug for ReadDir {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        // Only include the path field. This matches the Debug impl for
        // `std::fs::ReadDir`.
        write!(f, r#"ReadDir("{:?}")"#, self.path)
    }
}

// In pseudocode, here's what the iterator is doing:
//
// for block in file {
//   verify_checksum(block);
//   for dir_entry in block {
//     yield dir_entry;
//   }
// }
impl_result_iter!(ReadDir, DirEntry);

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::load_test_disk1;

    #[test]
    fn test_read_dir() {
        let fs = load_test_disk1();
        let root_inode = fs.read_root_inode().unwrap();
        let root_path = crate::PathBuf::new("/");

        // Use the iterator to get all DirEntries in the root directory.
        let entries: Vec<_> = ReadDir::new(fs, &root_inode, root_path)
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
}
