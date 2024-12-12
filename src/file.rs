// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::Ext4Error;
use crate::inode::Inode;
use crate::metadata::Metadata;
use crate::path::Path;
use crate::resolve::FollowSymlinks;
use crate::Ext4;
use core::fmt::{self, Debug, Formatter};

/// An open file within an [`Ext4`] filesystem.
pub struct File {
    inode: Inode,
}

impl File {
    /// Open the file at `path`.
    pub(crate) fn open(fs: &Ext4, path: Path<'_>) -> Result<Self, Ext4Error> {
        let inode = fs.path_to_inode(path, FollowSymlinks::All)?;

        if inode.metadata.is_dir() {
            return Err(Ext4Error::IsADirectory);
        }
        if !inode.metadata.file_type.is_regular_file() {
            return Err(Ext4Error::IsASpecialFile);
        }

        Ok(Self { inode })
    }

    /// Get the file metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.inode.metadata
    }
}

impl Debug for File {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("File")
            // Just show the inode index, the full `Inode` output is verbose.
            .field("inode", &self.inode.index)
            .finish_non_exhaustive()
    }
}
