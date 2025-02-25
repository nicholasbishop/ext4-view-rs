// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod block_header;
#[expect(unused)] // TODO
mod block_map;
mod commit_block;
mod descriptor_block;
#[expect(unused)] // TODO
mod superblock;

use crate::{Ext4, Ext4Error};

#[derive(Debug)]
pub(crate) struct Journal {
    // TODO: add journal data.
}

impl Journal {
    /// Create an empty journal.
    pub(crate) fn empty() -> Self {
        Self {}
    }

    /// Load a journal from the filesystem.
    pub(crate) fn load(fs: &Ext4) -> Result<Self, Ext4Error> {
        let Some(_journal_inode) = fs.0.superblock.journal_inode else {
            // Return an empty journal if this filesystem does not have
            // a journal.
            return Ok(Self::empty());
        };

        // TODO: actually load the journal.

        Ok(Self {})
    }
}
