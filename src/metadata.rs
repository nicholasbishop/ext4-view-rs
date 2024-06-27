// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::file_type::FileType;
use crate::inode::Inode;

/// Metadata information about a file.
pub struct Metadata {
    inode: Inode,
}

impl Metadata {
    pub(crate) fn new(inode: Inode) -> Self {
        Self { inode }
    }

    /// Get the file type.
    pub fn file_type(&self) -> FileType {
        self.inode.file_type
    }

    /// Return true if this metadata is for a directory.
    pub fn is_dir(&self) -> bool {
        self.inode.file_type.is_dir()
    }

    /// Return true if this metadata is for a symlink.
    pub fn is_symlink(&self) -> bool {
        self.inode.file_type.is_symlink()
    }

    /// Get the size in bytes of the file.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.inode.size_in_bytes
    }

    /// Get the file's UNIX permission bits.
    pub fn mode(&self) -> u32 {
        let mode = self.inode.mode.bits() & 0xfff;
        // Convert from u16 to u32 to match the std `PermissionsExt` interface.
        u32::from(mode)
    }
}
