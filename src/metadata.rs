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
}

impl Metadata {
    /// Get the file type.
    pub fn file_type(&self) -> FileType {
        self.file_type
    }

    /// Return true if this metadata is for a directory.
    pub fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    /// Return true if this metadata is for a symlink.
    pub fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// Get the size in bytes of the file.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.size_in_bytes
    }

    /// Get the file's UNIX permission bits.
    pub fn mode(&self) -> u32 {
        let mode = self.mode.bits() & 0xfff;
        // Convert from u16 to u32 to match the std `PermissionsExt` interface.
        u32::from(mode)
    }
}
