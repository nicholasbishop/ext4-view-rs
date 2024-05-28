// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::dir_entry::{DirEntry, DirEntryName};
use crate::error::{Corrupt, Ext4Error};
use crate::extent::{Extent, Extents};
use crate::inode::{Inode, InodeFlags, InodeIndex};
use crate::path::PathBuf;
use crate::util::{read_u16le, read_u32le, usize_from_u32};
use crate::Ext4;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;

/// Iterator over each [`DirEntry`] in a directory inode.
pub struct ReadDir<'a> {
    ext4: &'a Ext4,
    path: Rc<PathBuf>,
    inode: InodeIndex,
    checksum_base: Checksum,
    has_htree: bool,
    extents: Extents<'a>,
    extent: Option<Extent>,
    /// Index of the block within the extent.
    block_index: u64,
    block: Vec<u8>,
    offset_within_block: usize,
    is_done: bool,
}

impl<'a> ReadDir<'a> {
    pub(crate) fn new(
        ext4: &'a Ext4,
        inode: &Inode,
        path: PathBuf,
    ) -> Result<Self, Ext4Error> {
        // TODO: maybe just put this in the inode since it's used for
        // extents too?
        let mut checksum_base =
            Checksum::with_seed(ext4.superblock.checksum_seed);
        checksum_base.update_u32_le(inode.index.get());
        checksum_base.update_u32_le(inode.generation);

        let has_htree = inode.flags.contains(InodeFlags::DIRECTORY_HTREE);

        Ok(Self {
            ext4,
            path: Rc::new(path),
            inode: inode.index,
            checksum_base,
            has_htree,
            extents: Extents::new(ext4, inode)?,
            extent: None,
            block_index: 0,
            block: vec![0; usize_from_u32(ext4.superblock.block_size)],
            offset_within_block: 0,
            is_done: false,
        })
    }

    // Step to the next entry.
    //
    // This is factored out of `Iterator::next` for clarity and ease of
    // returning errors.
    //
    // When this returns `Ok(None)`, the outer loop in `Iterator::next`
    // will call it again until it reaches an actual value.
    fn next_impl(&mut self) -> Result<Option<DirEntry>, Ext4Error> {
        // Get the extent, or get the next one if not set.
        let extent = if let Some(extent) = &self.extent {
            extent
        } else {
            match self.extents.next() {
                Some(Ok(extent)) => {
                    self.extent = Some(extent);
                    self.block_index = 0;
                    self.offset_within_block = 0;

                    // OK to unwrap since we just set it.
                    self.extent.as_ref().unwrap()
                }
                Some(Err(err)) => return Err(err),
                None => {
                    self.is_done = true;
                    return Ok(None);
                }
            }
        };

        // If all blocks in the extent have been processed, move to the
        // next extent on the next iteration.
        if self.block_index == u64::from(extent.num_blocks) {
            self.extent = None;
            return Ok(None);
        }

        // If a block has been processed, move to the next block on the
        // next iteration.
        let block_size = self.ext4.superblock.block_size;
        if self.offset_within_block >= usize_from_u32(block_size) {
            self.block_index += 1;
            self.offset_within_block = 0;
            return Ok(None);
        }

        // If at the start of a new block, read it and verify the checksum.
        if self.offset_within_block == 0 {
            self.ext4.read_bytes(
                (self.block_index + extent.start_block) * u64::from(block_size),
                &mut self.block,
            )?;

            // TODO: dedup with DirEntry from_bytes
            let first_rec_len = u32::from(read_u16le(&self.block, 4));
            let is_internal_node = self.has_htree
                && (extent.block_within_file == 0
                    || first_rec_len == block_size);

            // Verify checksum of the whole directory block.
            if self.ext4.has_metadata_checksums() && !is_internal_node {
                let tail_entry_size = 12;
                let tail_entry_offset =
                    usize_from_u32(block_size) - tail_entry_size;
                let checksum_offset = tail_entry_offset + 8;
                let expected_checksum =
                    read_u32le(&self.block, checksum_offset);

                let mut checksum = self.checksum_base.clone();
                checksum.update(&self.block[..tail_entry_offset]);
                let actual_checksum = checksum.finalize();
                if expected_checksum != actual_checksum {
                    return Err(Ext4Error::Corrupt(Corrupt::DirBlockChecksum(
                        self.inode.get(),
                    )));
                }
            }
        }

        let (entry, entry_size) = DirEntry::from_bytes(
            &self.block[self.offset_within_block..],
            self.inode,
            self.path.clone(),
        )?;
        self.offset_within_block += entry_size;

        Ok(entry)
    }
}

impl<'a> Iterator for ReadDir<'a> {
    type Item = Result<DirEntry, Ext4Error>;

    fn next(&mut self) -> Option<Result<DirEntry, Ext4Error>> {
        // In psuedocode, here's what the iterator is doing:
        //
        // for extent in extents(inode) {
        //   for block in extent.blocks {
        //     verify_checksum(block);
        //     for dir_entry in block {
        //       yield dir_entry;
        //     }
        //   }
        // }

        loop {
            if self.is_done {
                return None;
            }

            match self.next_impl() {
                Ok(Some(entry)) => return Some(Ok(entry)),
                Ok(None) => {
                    // Continue.
                }
                Err(err) => {
                    self.is_done = true;
                    return Some(Err(err));
                }
            }
        }
    }
}

pub(crate) fn get_dir_entry_by_name(
    ext4: &Ext4,
    inode: &Inode,
    name: DirEntryName<'_>,
) -> Result<DirEntry, Ext4Error> {
    assert!(inode.file_type.is_dir());

    // TODO: add faster lookup by hash, if the inode has
    // InodeFlags::DIRECTORY_HTREE.

    // TODO
    let path = PathBuf::empty();
    for entry in ReadDir::new(ext4, inode, path)? {
        let entry = entry?;
        if entry.file_name() == name {
            return Ok(entry);
        }
    }

    Err(Ext4Error::NotFound)
}
