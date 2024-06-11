// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

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
