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
//!     ext4.read("/some/file/path").unwrap()
//! }
//! ```

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![forbid(unsafe_code)]
// TODO(nicholasbishop): Temporarily allow dead code to allow for
// smaller PRs.
#![allow(dead_code)]
#![warn(clippy::as_conversions)]
#![warn(missing_docs)]
#![warn(unreachable_pub)]

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
use alloc::string::String;
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

    /// Follow a path to get an inode.
    fn path_to_inode(&self, path: Path<'_>) -> Result<Inode, Ext4Error> {
        if !path.is_absolute() {
            return Err(Ext4Error::NotAbsolute);
        }

        let mut inode = self.read_root_inode()?;

        for component in path.components() {
            match component {
                // RootDir is only every returned by `components()` for
                // the first component, and `inode` is already the root
                // inode in that case.
                Component::RootDir => continue,

                // Nothing to do for `CurDir`.
                Component::CurDir => continue,

                // TODO: add support for "..".
                Component::ParentDir => {
                    return Err(Ext4Error::Incompatible(
                        Incompatible::UnresolvedPath,
                    ))
                }
                Component::Normal(name) => {
                    // TODO: add support for symlinks.
                    if inode.file_type.is_symlink() {
                        return Err(Ext4Error::Incompatible(
                            Incompatible::UnresolvedPath,
                        ));
                    }

                    // Lookup the entry in the directory.
                    if inode.file_type.is_dir() {
                        inode = dir::get_dir_entry_inode_by_name(
                            self, &inode, name,
                        )?;
                    } else {
                        // Can't look up a child of a non-directory;
                        // path is invalid. This handles a case like
                        // "/a/b", where "a" is a regular file instead
                        // of a directory.
                        return Err(Ext4Error::NotFound);
                    }
                }
            }
        }

        // Check if the final component is a symlink.
        // TODO: add support for symlinks.
        if inode.file_type.is_symlink() {
            return Err(Ext4Error::Incompatible(Incompatible::UnresolvedPath));
        }

        Ok(inode)
    }
}

/// These methods mirror the [`std::fs`][stdfs] API.
///
/// [stdfs]: https://doc.rust-lang.org/std/fs/index.html
impl Ext4 {
    /// Read the entire contents of a file as raw bytes.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    /// * `path` is a directory or special file type.
    pub fn read<'p, P>(&self, path: P) -> Result<Vec<u8>, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<Vec<u8>, Ext4Error> {
            let inode = fs.path_to_inode(path)?;

            if inode.file_type.is_dir() {
                return Err(Ext4Error::IsADirectory);
            }
            if !inode.file_type.is_regular_file() {
                return Err(Ext4Error::IsASpecialFile);
            }

            fs.read_inode_file(&inode)
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
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

    /// Get [`Metadata`] for `path`.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    pub fn metadata<'p, P>(&self, path: P) -> Result<Metadata, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<Metadata, Ext4Error> {
            let inode = fs.path_to_inode(path)?;
            Ok(Metadata::new(inode))
        }

        inner(self, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }
}

#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_inode() {
        let fs_path = std::path::Path::new("test_data/test_disk1.bin");
        let fs = Ext4::load_from_path(fs_path).unwrap();

        let inode = fs.path_to_inode(Path::try_from("/").unwrap()).unwrap();
        assert_eq!(inode.index.get(), 2);

        // Successful lookup.
        assert!(fs
            .path_to_inode(Path::try_from("/empty_file").unwrap())
            .is_ok());

        // Successful lookup with a "." component.
        assert!(fs
            .path_to_inode(Path::try_from("/./empty_file").unwrap())
            .is_ok());

        // Error: not an absolute path.
        assert!(fs
            .path_to_inode(Path::try_from("empty_file").unwrap())
            .is_err());

        // Error: invalid child of a valid directory.
        assert!(fs
            .path_to_inode(Path::try_from("/empty_dir/does_not_exist").unwrap())
            .is_err());

        // Error: attempted to lookup child of a regular file.
        assert!(fs
            .path_to_inode(
                Path::try_from("/empty_file/does_not_exist").unwrap()
            )
            .is_err());

        // Error: symlinks aren't supported yet.
        assert!(fs
            .path_to_inode(Path::try_from("/sym_simple").unwrap())
            .is_err());
        assert!(fs
            .path_to_inode(Path::try_from("/sym_simple/a").unwrap())
            .is_err());

        // Error: ".." isn't supported yet.
        assert!(fs
            .path_to_inode(Path::try_from("/empty_dir/..").unwrap())
            .is_err());

        // TODO: add deeper paths to the test disk and test here.
    }
}
