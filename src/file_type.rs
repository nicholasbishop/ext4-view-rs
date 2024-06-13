// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::inode::InodeMode;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileTypeError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FileType {
    BlockDevice,
    CharacterDevice,
    Directory,
    Fifo,
    Regular,
    Socket,
    Symlink,
}

impl FileType {
    pub fn is_block_dev(self) -> bool {
        self == FileType::BlockDevice
    }

    pub fn is_char_dev(self) -> bool {
        self == FileType::CharacterDevice
    }

    pub fn is_dir(self) -> bool {
        self == FileType::Directory
    }

    pub fn is_fifo(self) -> bool {
        self == FileType::Fifo
    }

    pub fn is_regular_file(self) -> bool {
        self == FileType::Regular
    }

    pub fn is_socket(self) -> bool {
        self == FileType::Socket
    }

    pub fn is_symlink(self) -> bool {
        self == FileType::Symlink
    }
}

impl TryFrom<InodeMode> for FileType {
    type Error = FileTypeError;

    fn try_from(mode: InodeMode) -> Result<Self, Self::Error> {
        // Mask out the lower bits.
        let mode = InodeMode::from_bits_retain(mode.bits() & 0xf000);

        if mode == InodeMode::S_IFIFO {
            Ok(Self::Fifo)
        } else if mode == InodeMode::S_IFCHR {
            Ok(Self::CharacterDevice)
        } else if mode == InodeMode::S_IFDIR {
            Ok(Self::Directory)
        } else if mode == InodeMode::S_IFBLK {
            Ok(Self::BlockDevice)
        } else if mode == InodeMode::S_IFREG {
            Ok(Self::Regular)
        } else if mode == InodeMode::S_IFLNK {
            Ok(Self::Symlink)
        } else if mode == InodeMode::S_IFSOCK {
            Ok(Self::Socket)
        } else {
            Err(FileTypeError)
        }
    }
}
