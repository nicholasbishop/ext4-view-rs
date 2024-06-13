// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::error::{Corrupt, Ext4Error};
use crate::inode::InodeIndex;
use crate::util::read_u16le;

/// Size of each entry within an extent node (including the header
/// entry).
const ENTRY_SIZE_IN_BYTES: usize = 12;

/// Contiguous range of blocks that contain file data.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Extent {
    // Offset of the block within the file.
    pub(crate) block_within_file: u32,

    // This is the actual block within the filesystem.
    pub(crate) start_block: u64,

    // Number of blocks (both within the file, and on the filesystem).
    pub(crate) num_blocks: u16,
}

/// Header at the start of a node in an extent tree.
///
/// An extent tree is made up of nodes. Each node may be internal or
/// leaf. Leaf nodes contain `Extent`s. Internal nodes point at other
/// nodes.
///
/// Each node starts with a `NodeHeader` that includes the node's depth
/// (depth 0 is a leaf node) and the number of entries in the node.
struct NodeHeader {
    /// Number of entries in this node, not including the header.
    num_entries: u16,

    /// Maximum number of entries in this node, not including the header.
    max_entries: u16,

    /// Depth of this node in the tree. Zero means it's a leaf node. The
    /// maximum depth is five.
    depth: u16,
}

impl NodeHeader {
    /// Size of the node, including the header.
    fn node_size_in_bytes(&self) -> usize {
        (usize::from(self.num_entries) + 1) * ENTRY_SIZE_IN_BYTES
    }

    /// Offset of the node's extent data.
    fn checksum_offset(&self) -> usize {
        (usize::from(self.max_entries) + 1) * ENTRY_SIZE_IN_BYTES
    }
}

impl NodeHeader {
    /// Read a `NodeHeader` from a byte slice.
    fn from_bytes(data: &[u8], inode: InodeIndex) -> Result<Self, Ext4Error> {
        if data.len() < ENTRY_SIZE_IN_BYTES {
            return Err(Ext4Error::Corrupt(Corrupt::ExtentNotEnoughData(
                inode.get(),
            )));
        }

        let eh_magic = read_u16le(data, 0);
        let eh_entries = read_u16le(data, 2);
        let eh_max = read_u16le(data, 4);
        let eh_depth = read_u16le(data, 6);

        if eh_magic != 0xf30a {
            return Err(Ext4Error::Corrupt(Corrupt::ExtentMagic(inode.get())));
        }

        if eh_depth > 5 {
            return Err(Ext4Error::Corrupt(Corrupt::ExtentDepth(inode.get())));
        }

        Ok(Self {
            depth: eh_depth,
            num_entries: eh_entries,
            max_entries: eh_max,
        })
    }
}
