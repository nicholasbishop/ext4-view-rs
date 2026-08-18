// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use crate::inode::InodeMode;

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

    /// Index of the inode this metadata came from.
    pub(crate) inode: u32,

    /// Number of hard links to the file.
    pub(crate) nlink: u16,

    /// Number of 512-byte sectors allocated to the file.
    pub(crate) blocks: u64,
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

    /// Index of the inode this metadata came from.
    ///
    /// Inode indices start at one, so this is never zero. Two paths with
    /// the same inode index refer to the same file (for example, hard
    /// links).
    #[must_use]
    pub fn inode(&self) -> u32 {
        self.inode
    }

    /// Number of hard links to the file.
    ///
    /// Note that when the file system has the `dir_nlink` feature and a
    /// directory has more than 64,999 subdirectories, the on-disk link
    /// count is set to one to indicate that the real count is unknown. In
    /// that case this method returns one.
    ///
    /// See `st_nlink` in [inode(7)][inode] for more details.
    ///
    /// [inode]: https://www.man7.org/linux/man-pages/man7/inode.7.html
    #[must_use]
    pub fn nlink(&self) -> u16 {
        self.nlink
    }

    /// Number of 512-byte sectors actually allocated to the file.
    ///
    /// This can be smaller than [`len`][Self::len] divided by 512 for a
    /// sparse file, and larger for a file with indirect blocks or extent
    /// tree nodes, since those count towards the allocation too.
    ///
    /// See `st_blocks` in [inode(7)][inode] for more details.
    ///
    /// [inode]: https://www.man7.org/linux/man-pages/man7/inode.7.html
    #[must_use]
    pub fn blocks(&self) -> u64 {
        self.blocks
    }
}
