// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::inode::Inode;
use crate::resolve::FollowSymlinks;
use crate::{Ext4, Ext4Error, Metadata, Path};

/// A read-only file in an [`Ext4`] filesystem.
pub struct Ext4File {
    fs: Ext4,
    inode: Inode,

    /// Current byte offset within the file.
    offset: u64,
}

impl Ext4File {
    /// Open a file.
    ///
    /// # Errors
    ///
    /// An error will be returned if:
    /// * `path` is not absolute.
    /// * `path` does not exist.
    /// * `path` is a directory or special file type.
    ///
    /// This is not an exhaustive list of errors, see the
    /// [crate documentation](crate#errors).
    pub fn open<'p, P>(fs: &Ext4, path: P) -> Result<Self, Ext4Error>
    where
        P: TryInto<Path<'p>>,
    {
        fn inner(fs: &Ext4, path: Path<'_>) -> Result<Ext4File, Ext4Error> {
            let inode = fs.path_to_inode(path, FollowSymlinks::All)?;

            if inode.metadata.is_dir() {
                return Err(Ext4Error::IsADirectory);
            }
            if !inode.metadata.file_type.is_regular_file() {
                return Err(Ext4Error::IsASpecialFile);
            }

            Ok(Ext4File {
                fs: fs.clone(),
                inode,
                offset: 0,
            })
        }

        inner(fs, path.try_into().map_err(|_| Ext4Error::MalformedPath)?)
    }

    /// Get the file metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.inode.metadata
    }

    /// Read some bytes from the file.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, Ext4Error> {}
}
