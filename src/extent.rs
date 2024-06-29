// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::error::{Corrupt, Ext4Error};
use crate::inode::{Inode, InodeIndex};
use crate::util::{read_u16le, read_u32le, u64_from_hilo};
use crate::Ext4;
use alloc::vec;
use alloc::vec::Vec;

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

#[derive(Clone)]
struct ToVisitItem {
    // Full node data. This starts with the node header, then some
    // number of entries. See [`NodeHeader`] for more on the structure
    // of extent tree data.
    node: Vec<u8>,

    // Current index within the node. 0 is the node header, 1 is the
    // first entry, etc.
    entry: u16,

    // Node depth, copied from the node header.
    depth: u16,
}

impl ToVisitItem {
    fn new(mut node: Vec<u8>, inode: InodeIndex) -> Result<Self, Ext4Error> {
        let header = NodeHeader::from_bytes(&node, inode)?;

        // The node data must be large enough to contain the number of
        // entries specified in the header.
        if node.len() < header.node_size_in_bytes() {
            return Err(Ext4Error::Corrupt(Corrupt::ExtentNotEnoughData(
                inode.get(),
            )));
        }

        // Remove unused data at the end (e.g. checksum data).
        node.truncate(header.node_size_in_bytes());

        Ok(Self {
            node,
            entry: 0,
            depth: header.depth,
        })
    }

    fn entry(&self) -> Option<&[u8]> {
        let start = usize::from(self.entry) * ENTRY_SIZE_IN_BYTES;
        self.node.get(start..start + ENTRY_SIZE_IN_BYTES)
    }
}

/// Iterator of an inode's extent tree.
pub(crate) struct Extents<'a> {
    ext4: &'a Ext4,
    inode: InodeIndex,
    to_visit: Vec<ToVisitItem>,
    checksum_base: Checksum,
}

impl<'a> Extents<'a> {
    pub(crate) fn new(
        ext4: &'a Ext4,
        inode: &Inode,
    ) -> Result<Self, Ext4Error> {
        Ok(Self {
            ext4,
            inode: inode.index,
            to_visit: vec![ToVisitItem::new(
                inode.inline_data.to_vec(),
                inode.index,
            )?],
            checksum_base: inode.checksum_base.clone(),
        })
    }

    // Step to the next entry.
    //
    // This is factored out of `Iterator::next` for clarity and ease of
    // returning errors.
    //
    // # Preconditions
    //
    // * The `to_visit` vec must not be empty when `next_impl` is called.
    //
    // # Returns
    //
    // * Returns `Ok(Some(_))` if the next entry is within a leaf node.
    // * Returns `Err` if a hard error occurs (this will be returned by
    //   the iterator, and iteration will be ended on the next
    //   iteration).
    // * Returns `Ok(None)` for other cases. This doesn't end iteration,
    //   just means the iterator is not in a leaf node. The outer loop
    //   in `Iterator::next` will call `next_impl` again as long as
    //   there are nodes left to process.
    fn next_impl(&mut self) -> Result<Option<Extent>, Ext4Error> {
        // OK to unwrap: see preconditions.
        let item = self.to_visit.last_mut().unwrap();
        // Increment at the start to ensure that early returns don't
        // accidentally skip the increment. Since entry 0 is the node
        // header, entry 1 is the first actual node entry.
        item.entry += 1;

        let Some(entry) = &item.entry() else {
            // Reached end of this node.
            self.to_visit.pop();
            return Ok(None);
        };

        if item.depth == 0 {
            let ee_block = read_u32le(entry, 0);
            let ee_len = read_u16le(entry, 4);
            let ee_start_hi = read_u16le(entry, 6);
            let ee_start_low = read_u32le(entry, 8);

            let start_block =
                u64_from_hilo(u32::from(ee_start_hi), ee_start_low);

            return Ok(Some(Extent {
                block_within_file: ee_block,
                start_block,
                num_blocks: ee_len,
            }));
        } else {
            let ei_leaf_lo = read_u32le(entry, 4);
            let ei_leaf_hi = read_u16le(entry, 8);
            let child_block = u64_from_hilo(u32::from(ei_leaf_hi), ei_leaf_lo);

            // Read just the header of the child node. This is needed to
            // find out how much data is in the full child node.
            let mut child_header = [0; ENTRY_SIZE_IN_BYTES];
            let child_start =
                child_block * u64::from(self.ext4.superblock.block_size);
            self.ext4.read_bytes(child_start, &mut child_header)?;
            let child_header =
                NodeHeader::from_bytes(&child_header, self.inode)?;

            // The checksum is written in the four bytes directly after
            // the node.
            let checksum_offset = child_header.checksum_offset();
            let checksum_size = if self.ext4.has_metadata_checksums() {
                4
            } else {
                0
            };

            let mut child_node = vec![0; checksum_offset + checksum_size];
            self.ext4.read_bytes(child_start, &mut child_node)?;

            // Validating the checksum here covers everything but the
            // root node. The root node is embedded within the inode,
            // which has its own checksum.
            if self.ext4.has_metadata_checksums() {
                let expected_checksum =
                    read_u32le(&child_node, checksum_offset);

                let mut checksum = self.checksum_base.clone();
                checksum.update(&child_node[..checksum_offset]);
                let actual_checksum = checksum.finalize();
                if expected_checksum != actual_checksum {
                    return Err(Ext4Error::Corrupt(Corrupt::ExtentChecksum(
                        self.inode.get(),
                    )));
                }
            }

            self.to_visit
                .push(ToVisitItem::new(child_node, self.inode)?);
        }

        // This does not indicate end of iteration, we just haven't
        // reached a leaf node yet.
        Ok(None)
    }
}

impl<'a> Iterator for Extents<'a> {
    type Item = Result<Extent, Ext4Error>;

    fn next(&mut self) -> Option<Result<Extent, Ext4Error>> {
        while !self.to_visit.is_empty() {
            match self.next_impl() {
                Ok(Some(extent)) => return Some(Ok(extent)),
                Ok(None) => {
                    // continue
                }
                Err(err) => {
                    // Clear `to_visit` so that future calls to `next`
                    // return `None`.
                    self.to_visit.clear();
                    return Some(Err(err));
                }
            }
        }

        None
    }
}
