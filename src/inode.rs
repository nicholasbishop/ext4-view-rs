// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use bitflags::bitflags;
use core::num::NonZeroU32;

/// Inode index.
///
/// This is always nonzero.
pub(crate) type InodeIndex = NonZeroU32;

bitflags! {
    /// Inode flags.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub(crate) struct InodeFlags: u32 {
        /// File is immutable.
        const IMMUTABLE = 0x10;

        /// Directory has hashed indexes.
        const DIRECTORY_HTREE = 0x1000;

        /// File is huge.
        const HUGE_FILE = 0x4_0000;

        /// Inode uses extents.
        const EXTENTS = 0x8_0000;

        /// Verity protected data.
        const VERITY = 0x10_0000;

        /// Inode stores a large extended attribute value in its data blocks.
        const EXTENDED_ATTRIBUTES = 0x20_0000;

        /// Inode has inline data.
        const INLINE_DATA = 0x1000_0000;

        // TODO: other flags
    }
}

bitflags! {
    /// Inode mode.
    ///
    /// The mode bitfield stores file permissions in the lower bits and
    /// file type in the upper bits.
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub(crate) struct InodeMode: u16 {
        const S_IXOTH = 0x0001;
        const S_IWOTH = 0x0002;
        const S_IROTH = 0x0004;

        const S_IXGRP = 0x0008;
        const S_IWGRP = 0x0010;
        const S_IRGRP = 0x0020;

        const S_IXUSR = 0x0040;
        const S_IWUSR = 0x0080;
        const S_IRUSR = 0x0100;

        const S_ISVTX = 0x0200;

        const S_ISGID = 0x0400;
        const S_ISUID = 0x0800;

        // Mutually-exclusive file types:
        const S_IFIFO = 0x1000;
        const S_IFCHR = 0x2000;
        const S_IFDIR = 0x4000;
        const S_IFBLK = 0x6000;
        const S_IFREG = 0x8000;
        const S_IFLNK = 0xA000;
        const S_IFSOCK = 0xC000;
    }
}

#[derive(Debug)]
pub(crate) struct Inode {
    /// This inode's index.
    pub(crate) index: InodeIndex,

    /// Various kinds of file data can be stored within the inode, including:
    /// * The root node of the extent tree.
    /// * Target path for symlinks.
    pub(crate) inline_data: [u8; Self::INLINE_DATA_LEN],

    /// Size in bytes of the file data.
    pub(crate) size_in_bytes: u64,

    /// Raw permissions and file type.
    pub(crate) mode: InodeMode,

    /// File type parsed from the `mode` bitfield.
    pub(crate) file_type: FileType,

    /// Internal inode flags.
    pub(crate) flags: InodeFlags,

    /// File version, usually zero. Used in checksums.
    pub(crate) generation: u32,

    checksum: u32,
}

impl Inode {
    const INLINE_DATA_LEN: usize = 60;
}
