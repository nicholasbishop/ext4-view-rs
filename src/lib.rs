// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![forbid(unsafe_code)]
// TODO(nicholasbishop): Temporarily allow dead code to allow for
// smaller PRs.
#![allow(dead_code)]

extern crate alloc;

mod block_group;
mod checksum;
mod dir;
mod dir_entry;
mod dir_entry_hash;
mod error;
mod extent;
mod features;
mod file_type;
mod format;
mod inode;
mod metadata;
mod path;
mod reader;
mod superblock;
mod util;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use block_group::BlockGroupDescriptor;
use core::cell::RefCell;
use extent::Extents;
use features::ReadOnlyCompatibleFeatures;
use inode::{Inode, InodeIndex};
use superblock::Superblock;
use util::usize_from_u32;

pub use dir::ReadDir;
pub use dir_entry::{DirEntry, DirEntryName, DirEntryNameError};
pub use error::{Corrupt, Ext4Error, Incompatible, IoError};
pub use features::IncompatibleFeatures;
pub use file_type::FileType;
pub use metadata::Metadata;
pub use path::{Component, Components, Path, PathBuf, PathError};
pub use reader::{Ext4Read, MemIoError};

/// Read-only access to an [ext4] filesystem.
///
/// [ext4]: https://en.wikipedia.org/wiki/Ext4
pub struct Ext4 {
    superblock: Superblock,
    block_group_descriptors: Vec<BlockGroupDescriptor>,

    /// Reader providing access to the underlying storage.
    ///
    /// Stored as `Box<dyn Ext4Read>` rather than a generic type to make
    /// the `Ext4` type more convenient to pass around for users of the API.
    ///
    /// The `Ext4Read::read` method takes `&mut self`, because readers
    /// like `std::fs::File` are mutable. However, the `Ext4` API is
    /// logically const -- it provides read-only access to the
    /// filesystem. So the box is wrapped in `RefCell` to allow the
    /// mutable method to be called with an immutable `&Ext4`
    /// reference. `RefCell` enforces at runtime that only one mutable
    /// borrow exists at a time.
    reader: RefCell<Box<dyn Ext4Read>>,
}

impl Ext4 {
    /// Load an `Ext4` instance from the given `reader`.
    ///
    /// This reads and validates the superblock and block group
    /// descriptors. No other data is read.
    pub fn load(mut reader: Box<dyn Ext4Read>) -> Result<Self, Ext4Error> {
        // The first 1024 bytes are reserved for "weird" stuff like x86
        // boot sectors.
        let superblock_start = 1024;
        let mut data = vec![0; Superblock::SIZE_IN_BYTES_ON_DISK];
        reader
            .read(superblock_start, &mut data)
            .map_err(Ext4Error::Io)?;

        let superblock = Superblock::from_bytes(&data)?;

        let mut ext4 = Self {
            reader: RefCell::new(reader),
            block_group_descriptors: Vec::with_capacity(usize_from_u32(
                superblock.num_block_groups,
            )),
            superblock,
        };

        // Read all the block group descriptors.
        for bgd_index in 0..ext4.superblock.num_block_groups {
            let bgd = BlockGroupDescriptor::read(&ext4, bgd_index)?;
            ext4.block_group_descriptors.push(bgd);
        }

        Ok(ext4)
    }

    /// Load an `Ext4` filesystem from the given `path`.
    ///
    /// This reads and validates the superblock and block group
    /// descriptors. No other data is read.
    #[cfg(feature = "std")]
    pub fn load_from_path(path: &std::path::Path) -> Result<Self, Ext4Error> {
        let file = std::fs::File::open(path)
            .map_err(|e| Ext4Error::Io(Box::new(e)))?;
        Self::load(Box::new(file))
    }

    /// Return true if the filesystem has metadata checksums enabled,
    /// false otherwise.
    fn has_metadata_checksums(&self) -> bool {
        self.superblock
            .read_only_compatible_features
            .contains(ReadOnlyCompatibleFeatures::METADATA_CHECKSUMS)
    }

    /// Read the inode of the root `/` directory.
    fn read_root_inode(&self) -> Result<Inode, Ext4Error> {
        let root_inode_index = InodeIndex::new(2).unwrap();
        Inode::read(self, root_inode_index)
    }

    /// Read bytes into `dst`, starting at `start_byte`.
    fn read_bytes(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Ext4Error> {
        self.reader
            .borrow_mut()
            .read(start_byte, dst)
            .map_err(Ext4Error::Io)
    }

    /// Read the entire contents of a file into a `Vec<u8>`.
    ///
    /// Holes are filled with zero.
    ///
    /// Fails with `FileTooLarge` if the size of the file is too large
    /// to fit in a [`usize`].
    fn read_inode_file(&self, inode: &Inode) -> Result<Vec<u8>, Ext4Error> {
        let block_size = self.superblock.block_size;

        // Get the file size and preallocate the output vector.
        let file_size_in_bytes = usize::try_from(inode.size_in_bytes)
            .map_err(|_| Ext4Error::FileTooLarge)?;
        let mut dst = vec![0; file_size_in_bytes];

        for extent in Extents::new(self, inode)? {
            let extent = extent?;

            let dst_start =
                usize_from_u32(extent.block_within_file * block_size);

            // Get the length (in bytes) of the extent.
            //
            // This length may actually be too long, since the last
            // block may extend past the end of the file. This is
            // checked below.
            let len = usize_from_u32(block_size * u32::from(extent.num_blocks));
            let dst_end = dst_start + len;
            // Cap to the end of the file.
            let dst_end = dst_end.min(file_size_in_bytes);

            let dst = &mut dst[dst_start..dst_end];

            let src_start = extent.start_block * u64::from(block_size);

            self.read_bytes(src_start, dst)?;
        }
        Ok(dst)
    }
}
