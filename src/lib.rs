// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This crate provides read-only access to [ext4] filesystems.
//!
//! [ext4]: https://en.wikipedia.org/wiki/Ext4
//!
//! # Example
//!
//! Load an ext4 filesystem from an in-memory buffer and then read a file
//! from the filesystem:
//!
//! ```
//! use ext4_view::{Ext4, Path};
//!
//! fn in_memory_example(fs_data: Vec<u8>) -> Vec<u8> {
//!     let ext4 = Ext4::load(Box::new(fs_data)).unwrap();
//!     let file_path = Path::try_from("/some/file/path").unwrap();
//!     ext4.read(file_path).unwrap()
//! }
//! ```

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
mod walk;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use block_group::BlockGroupDescriptor;
use core::cell::RefCell;
use extent::Extents;
use inode::{Inode, InodeIndex, LookupInode};
use util::usize_from_u32;

pub use dir::ReadDir;
pub use dir_entry::{DirEntry, DirEntryName};
pub use error::{Corrupt, Ext4Error, Incompatible, IoError};
pub use features::{IncompatibleFeatures, ReadOnlyCompatibleFeatures};
pub use metadata::Metadata;
pub use path::{Component, Components, Path, PathBuf, PathError};
pub use reader::Ext4Read;
pub use superblock::Superblock;
pub use walk::WalkIter;

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

    /// Get the superblock.
    pub fn superblock(&self) -> &Superblock {
        &self.superblock
    }

    /// Return true if the filesystem has metadata checksums enabled,
    /// false otherwise.
    fn has_metadata_checksums(&self) -> bool {
        self.superblock
            .read_only_compatible_features
            .contains(ReadOnlyCompatibleFeatures::METADATA_CHECKSUMS)
    }

    #[inline]
    fn block_size(&self) -> u32 {
        self.superblock.block_size
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

    fn read_inode(&self, inode: InodeIndex) -> Result<Inode, Ext4Error> {
        Inode::read(self, inode)
    }

    /// Get an iterator of an inode's extents.
    fn extents(&self, inode: &Inode) -> Result<Extents, Ext4Error> {
        Extents::new(self, inode)
    }

    // TODO: we'll want a streaming read as well, but for now just read
    // all at once.
    fn read_inode_file(&self, inode: &Inode) -> Result<Vec<u8>, Ext4Error> {
        // TODO: use iter
        let extents: Result<Vec<_>, Ext4Error> = self.extents(inode)?.collect();
        let extents = extents?;
        let file_size_in_bytes = usize::try_from(inode.size_in_bytes)
            .map_err(|_| Ext4Error::FileTooLarge)?;
        let mut output = vec![0; file_size_in_bytes];

        for extent in extents {
            let dst_start =
                usize_from_u32(extent.block_within_file * self.block_size());

            // This length may actually be too long, since the last
            // block may extend past the end of the file. This is
            // checked below.
            let len = usize_from_u32(
                self.block_size() * u32::from(extent.num_blocks),
            );
            let dst_end = dst_start + len;
            // Cap to the end of the file.
            let dst_end = dst_end.min(file_size_in_bytes);

            let dst = &mut output[dst_start..dst_end];

            let src_start = extent.start_block * u64::from(self.block_size());

            self.read_bytes(src_start, dst)?;
        }
        Ok(output)
    }

    pub fn walk(&self) -> WalkIter {
        WalkIter::new(self)
    }

    fn read_root_inode(&self) -> Result<Inode, Ext4Error> {
        self.read_inode(InodeIndex::new(2).unwrap())
    }

    // TODO: this is definitely not correct yet.
    fn path_to_inode(&self, path: Path<'_>) -> Result<Inode, Ext4Error> {
        if !path.is_absolute() {
            return Err(Ext4Error::NotAbsolute);
        }

        let mut inode = self.read_root_inode()?;

        // TODO: think about "." / "..", symlinks, etc.
        for component in path.components() {
            match component {
                Component::RootDir => continue,
                Component::CurDir => continue,
                Component::ParentDir => todo!(),
                Component::Normal(name) => {
                    if inode.file_type.is_dir() {
                        let entry =
                            dir::get_dir_entry_by_name(self, &inode, name)?;
                        inode = self.read_inode(entry.inode())?;
                    }
                }
            }
        }

        Ok(inode)
    }
}

/// These methods mirror the [`std::fs`][stdfs] API.
///
/// [stdfs]: https://doc.rust-lang.org/std/fs/index.html
impl Ext4 {
    // The following methods take a generic path parameter (making it
    // simpler to call the functions with a simple string, for example).
    //
    // The non-generic inner function pattern is used:
    // (https://www.possiblerust.com/pattern/non-generic-inner-functions)

    /// Get the canonical, absolute form of a path with all intermediate
    /// components normalized and symbolic links resolved.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    pub fn canonicalize<'p, P>(&self, path: P) -> Result<PathBuf, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<PathBuf, Ext4Error> {
            if !path.is_absolute() {
                return Err(Ext4Error::NotAbsolute);
            }

            let mut inode = fs.read_root_inode()?;
            // let num_components = path.components().count();
            let mut output_path = PathBuf::from(Path::ROOT);

            // TODO: think about "." / "..", symlinks, etc.
            for component in path.components() {
                match component {
                    Component::RootDir => continue,
                    Component::CurDir => continue,
                    Component::ParentDir => todo!(),
                    Component::Normal(name) => {
                        if inode.file_type.is_dir() {
                            let entry =
                                dir::get_dir_entry_by_name(fs, &inode, name)?;
                            inode = fs.read_inode(entry.inode())?;
                            output_path.push(name);
                        }
                    }
                }
            }

            Ok(output_path)
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }

    /// Read the entire contents of a file as raw bytes.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    /// * `path` is a directory or special file type.
    pub fn read<S: LookupInode>(&self, src: S) -> Result<Vec<u8>, Ext4Error> {
        fn inner(fs: &Ext4, inode: Inode) -> Result<Vec<u8>, Ext4Error> {
            if inode.file_type.is_dir() {
                return Err(Ext4Error::IsADirectory);
            }
            if !inode.file_type.is_regular_file() {
                return Err(Ext4Error::IsASpecialFile);
            }

            // TODO: drop separate function
            fs.read_inode_file(&inode)
        }

        let inode = src.lookup_inode(self)?;
        inner(self, inode)
    }

    /// Read the entire contents of a file as a string.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    /// * `path` is a directory or special file type.
    pub fn read_to_string<'p, P>(&self, path: P) -> Result<String, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<String, Ext4Error> {
            let content = fs.read(path)?;
            String::from_utf8(content).map_err(|_| Ext4Error::NotUtf8)
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }

    /// Get the target of a symbolic link.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    /// * `path` is not a symlink.
    pub fn read_link<S: LookupInode>(
        &self,
        src: S,
    ) -> Result<PathBuf, Ext4Error> {
        let inode = src.lookup_inode(self)?;
        inode.symlink_target(self)
    }

    /// Get an iterator over the entries in a directory.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist
    /// * `path` is not a directory
    pub fn read_dir<'p, P>(&self, path: P) -> Result<ReadDir, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner<'a>(
            fs: &'a Ext4,
            path: Path<'_>,
        ) -> Result<ReadDir<'a>, Ext4Error> {
            let inode = fs.path_to_inode(path)?;

            if !inode.file_type.is_dir() {
                return Err(Ext4Error::NotADirectory);
            }

            ReadDir::new(fs, &inode, path.into())
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }

    /// Check if `path` exists.
    ///
    /// Returns `Ok(true)` if `path` exists, or `Ok(false)` if it does
    /// not exist.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    pub fn exists<'p, P>(&self, path: P) -> Result<bool, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<bool, Ext4Error> {
            match fs.path_to_inode(path) {
                Ok(_) => Ok(true),
                Err(Ext4Error::NotFound) => Ok(false),
                Err(err) => Err(err),
            }
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }

    pub fn metadata<S: LookupInode>(
        &self,
        src: S,
    ) -> Result<Metadata, Ext4Error> {
        let inode = src.lookup_inode(self)?;
        Ok(Metadata::new(inode))
    }

    // TODO: symlink_metadata()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disk1() {
        let data = include_bytes!("../test_data/test_disk1.bin");
        let ext4 = Ext4::load(Box::new(data.as_slice())).unwrap();
        // TODO: figure out what all we actually care about testing
        // here.
        assert_eq!(ext4.superblock.block_size, 1024);
        assert_eq!(ext4.superblock.blocks_count, 65_536);
        assert_eq!(ext4.superblock.inodes_per_block_group, 2048);
        assert_eq!(ext4.superblock.num_block_groups, 8);
        let dir = ext4
            .read_dir(Path::ROOT)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(
            dir.iter()
                .map(|e| e.file_name().as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                ".",
                "..",
                "lost+found",
                "empty_file",
                "empty_dir",
                "small_file",
                "sym_simple",
                "sym_59",
                "sym_60",
                "big_dir",
                "holes"
            ]
        );

        // Check contents of path `/small_file`.
        {
            let small_file_data = ext4.read_to_string("/small_file").unwrap();
            assert_eq!(small_file_data, "hello, world!");
        }

        // Check the targets of the symlinks.
        {
            assert_eq!(ext4.read_link("/sym_simple").unwrap(), "small_file");

            // Symlink target is inline.
            assert_eq!(ext4.read_link("/sym_59").unwrap(), "a".repeat(59));

            // Symlink target is stored in extents.
            assert_eq!(ext4.read_link("/sym_60").unwrap(), "a".repeat(60));

            // Not a symlink.
            assert!(matches!(
                ext4.read_link("/small_file").unwrap_err(),
                Ext4Error::NotASymlink
            ));
        }

        // Check contents of directory `/big_dir`.
        {
            let dir = ext4
                .read_dir("/big_dir")
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            let mut entry_names: Vec<String> = dir
                .iter()
                .map(|e| e.file_name().as_str().unwrap().to_owned())
                .collect();
            entry_names.sort_unstable();

            let mut entry_paths: Vec<PathBuf> =
                dir.iter().map(|e| e.path()).collect();
            entry_paths.sort_unstable();

            let mut expected_names = vec![".".to_owned(), "..".to_owned()];
            expected_names.extend((0u32..10_000u32).map(|n| n.to_string()));
            expected_names.sort_unstable();

            let expected_paths = expected_names
                .iter()
                .map(|n| {
                    PathBuf::try_from(format!("/big_dir/{n}").as_bytes())
                        .unwrap()
                })
                .collect::<Vec<_>>();

            assert_eq!(entry_names, expected_names);
            assert_eq!(entry_paths, expected_paths);
        }

        // Check contents of `/holes`.
        {
            let data = ext4.read("/holes").unwrap();
            let mut expected = vec![];
            for i in 0..5 {
                expected.extend(vec![0xa5; 4096]);
                if i != 4 {
                    expected.extend(vec![0; 8192]);
                }
            }
            assert_eq!(data, expected);
        }

        for _entry in ext4.walk() {
            // TODO: for now just verifying that walk does not error.
        }
    }
}
