// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use crate::inode::InodeMode;
use crate::timestamp::Timestamp;

/// Metadata information about a file.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Metadata {
    /// Size in bytes of the file data.
    pub(crate) size_in_bytes: u64,

    /// Raw permissions and file type.
    pub(crate) mode: InodeMode,

    /// File type parsed from the `mode` bitfield.
    pub(crate) file_type: FileType,

    /// Owner user ID.
    pub(crate) uid: u32,

    /// Owner group ID.
    pub(crate) gid: u32,

    /// Time of last access.
    pub(crate) atime: Timestamp,

    /// Time of last inode change.
    pub(crate) ctime: Timestamp,

    /// Time of last content modification.
    pub(crate) mtime: Timestamp,

    /// Time of creation. `None` for inodes too small to store it.
    pub(crate) crtime: Option<Timestamp>,
}

impl Metadata {
    /// Get the file type.
    #[must_use]
    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    /// Return true if this metadata is for a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// Return true if this metadata is for a symlink.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// Get the size in bytes of the file.
    #[allow(clippy::len_without_is_empty)]
    #[must_use]
    pub fn len(&self) -> u64 {
        self.size_in_bytes
    }

    /// Get the file's UNIX permission bits.
    ///
    /// Diagram of the returned value's bits:
    ///
    /// ```text
    ///       top four bits are always zero
    ///       │   
    ///       │   set uid, set gid, sticky bit
    ///       │   │  
    ///       │   │  owner read/write/execute
    ///       │   │  │  
    ///       │   │  │  group read/write/execute
    ///       │   │  │  │  
    ///       │   │  │  │  other read/write/execute
    ///       │   │  │  │  │
    /// (msb) 0000xxxuuugggooo (lsb)
    /// ```
    ///
    /// See `st_mode` in [inode(7)][inode] for more details.
    ///
    /// [inode]: https://www.man7.org/linux/man-pages/man7/inode.7.html
    #[must_use]
    pub fn mode(&self) -> u16 {
        self.mode.bits() & 0o7777
    }

    /// Owner user ID.
    #[must_use]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Owner group ID.
    #[must_use]
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// Time of last access.
    ///
    /// Note that many systems are mounted with `noatime` or `relatime`, in
    /// which case this is not updated on every read.
    #[must_use]
    pub fn atime(&self) -> Timestamp {
        self.atime
    }

    /// Time the inode was last changed.
    ///
    /// This covers metadata changes such as permissions, in addition to
    /// content changes.
    #[must_use]
    pub fn ctime(&self) -> Timestamp {
        self.ctime
    }

    /// Time the file's contents were last modified.
    #[must_use]
    pub fn mtime(&self) -> Timestamp {
        self.mtime
    }

    /// Time the file was created.
    ///
    /// Returns `None` when the inode is too small to store a creation time.
    /// Only inodes larger than 128 bytes have this field, so file systems
    /// created with the minimum inode size do not record it.
    #[must_use]
    pub fn crtime(&self) -> Option<Timestamp> {
        self.crtime
    }
}
