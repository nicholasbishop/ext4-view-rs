// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod block_map;
mod extents_blocks;

use crate::inode::{Inode, InodeFlags};
use crate::{Ext4, Ext4Error};
use block_map::BlockMap;
use extents_blocks::ExtentsBlocks;

// This enum is separate from `FileBlocks` to keep the implementation
// details private to this module; members of an enum cannot be more
// private than the enum itself.
enum FileBlocksInner {
    ExtentsBlocks(ExtentsBlocks),
    BlockMap(BlockMap),
}

/// Iterator over blocks in a file.
///
/// The iterator produces absolute block indices. A block index of zero
/// indicates a hole.
///
/// TODO: files represented with extents do not currently yield anything
/// for holes. Those blocks are silently skipped over.
pub(crate) struct FileBlocks(FileBlocksInner);

impl FileBlocks {
    pub(crate) fn new(fs: Ext4, inode: &Inode) -> Result<Self, Ext4Error> {
        if inode.flags.contains(InodeFlags::EXTENTS) {
            Ok(Self(FileBlocksInner::ExtentsBlocks(ExtentsBlocks::new(
                fs, inode,
            )?)))
        } else {
            Ok(Self(FileBlocksInner::BlockMap(BlockMap::new(fs, inode))))
        }
    }
}

impl Iterator for FileBlocks {
    /// Block index.
    type Item = Result<u64, Ext4Error>;

    fn next(&mut self) -> Option<Result<u64, Ext4Error>> {
        match self {
            Self(FileBlocksInner::ExtentsBlocks(iter)) => iter.next(),
            Self(FileBlocksInner::BlockMap(iter)) => iter.next(),
        }
    }
}
