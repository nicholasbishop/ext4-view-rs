// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod block_header;
mod descriptor_block;
mod superblock;

use crate::checksum::Checksum;
use crate::error::{CorruptKind, Ext4Error, IncompatibleKind};
use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::util::{read_u32be, usize_from_u32};
use crate::Ext4;
use alloc::collections::BTreeMap;
use alloc::vec;
use block_header::{JournalBlockHeader, JournalBlockType};
use descriptor_block::{
    validate_descriptor_block_checksum, DescriptorBlockTagIter,
};
use superblock::JournalSuperblock;

#[derive(Debug)]
pub(crate) struct Journal {
    block_map: BTreeMap<u64, u64>,
}

impl Journal {
    pub(crate) fn empty() -> Self {
        Self {
            block_map: BTreeMap::new(),
        }
    }

    /// Load the journal.
    ///
    /// If the filesystem has no journal, an empty journal is returned.
    ///
    /// Note: ext4 is all little-endian, except for the journal, which
    /// is all big-endian.
    pub(crate) fn load(fs: &Ext4) -> Result<Self, Ext4Error> {
        let Some(journal_inode) = fs.0.superblock.journal_inode else {
            // Return an empty journal if this filesystem does not have
            // a journal.
            return Ok(Self::empty());
        };

        let journal_inode = Inode::read(fs, journal_inode)?;
        let superblock = JournalSuperblock::load(fs, &journal_inode)?;

        // Ensure the journal block size matches the rest of the
        // filesystem.
        let block_size = fs.0.superblock.block_size;
        if superblock.block_size != block_size {
            return Err(CorruptKind::JournalBlockSize.into());
        }

        let block_map = load_block_map(fs, &superblock, &journal_inode)?;

        Ok(Self { block_map })
    }

    /// Map from an absolute block index to a block in the journal.
    ///
    /// If the journal does not contain a replacement for the input
    /// block, the input block is returned.
    pub(crate) fn map_block_index(&self, block_index: u64) -> u64 {
        *self.block_map.get(&block_index).unwrap_or(&block_index)
    }
}

fn load_block_map(
    fs: &Ext4,
    superblock: &JournalSuperblock,
    journal_inode: &Inode,
) -> Result<BTreeMap<u64, u64>, Ext4Error> {
    // Get an iterator over the journal's block indices.
    let journal_block_iter = FileBlocks::new(fs.clone(), journal_inode)?;

    // Skip forward to the start block.
    let mut journal_block_iter =
        journal_block_iter.skip(usize_from_u32(superblock.start_block));

    // TODO: the loop below currently returns an error if something
    // bad is encountered (e.g. a wrong checksum). We should
    // actually still apply valid commits, and just stop reading the
    // journal when bad data is encountered.

    let mut block = vec![0; fs.0.superblock.block_size.to_usize()];
    let mut data_block = vec![0; fs.0.superblock.block_size.to_usize()];
    let mut block_map = BTreeMap::new();
    let mut uncommitted_block_map = BTreeMap::new();
    let mut sequence = superblock.sequence;
    while let Some(block_index) = journal_block_iter.next() {
        let block_index = block_index?;

        fs.read_from_block(block_index, 0, &mut block)?;

        let Some(header) = JournalBlockHeader::read_bytes(&block)? else {
            // Journal block magic is not present, so we've reached
            // the end of the journal.
            break;
        };

        if header.sequence != sequence {
            return Err(CorruptKind::JournalSequence.into());
        }

        if header.block_type == JournalBlockType::DESCRIPTOR {
            validate_descriptor_block_checksum(superblock, &block)?;

            let tags = DescriptorBlockTagIter::new(&block[12..]);

            for tag in tags {
                let tag = tag?;

                let block_index = journal_block_iter
                    .next()
                    .ok_or(CorruptKind::JournalTruncated)??;

                // Check the data block checksum.
                let mut checksum = Checksum::new();
                checksum.update(superblock.uuid.as_bytes());
                checksum.update_u32_be(sequence);
                fs.read_from_block(block_index, 0, &mut data_block)?;
                checksum.update(&data_block);
                if checksum.finalize() != tag.checksum {
                    return Err(
                        CorruptKind::JournalDescriptorTagChecksum.into()
                    );
                }

                uncommitted_block_map.insert(tag.block_index, block_index);
            }
        } else if header.block_type == JournalBlockType::COMMIT {
            validate_commit_block_checksum(superblock, &block)?;

            // Move the entries from `uncommitted_block_map` to `block_map`.
            block_map.extend(uncommitted_block_map.iter());
            uncommitted_block_map.clear();

            sequence = sequence
                .checked_add(1)
                .ok_or(CorruptKind::JournalSequenceOverflow)?;
        } else {
            return Err(IncompatibleKind::JournalBlockType(
                header.block_type.0,
            )
            .into());
        }
    }

    Ok(block_map)
}

fn validate_commit_block_checksum(
    superblock: &JournalSuperblock,
    block: &[u8],
) -> Result<(), Ext4Error> {
    // The kernel documentation says that fields 0xc and 0xd contain the
    // checksum type and size, but this is not correct. If the
    // superblock features include `CHECKSUM_V3`, the type/size fields
    // are both zero.

    const CHECKSUM_OFFSET: usize = 16;
    const CHECKSUM_SIZE: usize = 4;

    let expected_checksum = read_u32be(block, CHECKSUM_OFFSET);

    let mut checksum = Checksum::new();
    checksum.update(superblock.uuid.as_bytes());
    checksum.update(&block[..CHECKSUM_OFFSET]);
    checksum.update(&[0; CHECKSUM_SIZE]);
    checksum.update(&block[CHECKSUM_OFFSET + CHECKSUM_SIZE..]);

    if checksum.finalize() == expected_checksum {
        Ok(())
    } else {
        Err(CorruptKind::JournalCommitBlockChecksum.into())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use crate::test_util::load_compressed_filesystem;
    use alloc::rc::Rc;

    #[test]
    fn test_journal() {
        let mut fs =
            load_compressed_filesystem("test_disk_4k_block_journal.bin.zst");

        let test_dir = "/dir500";

        // With the journal in place, this directory exists.
        assert!(fs.exists(test_dir).unwrap());

        // Clear the journal, and verify that the directory no longer exists.
        Rc::get_mut(&mut fs.0).unwrap().journal.block_map.clear();
        assert!(!fs.exists(test_dir).unwrap());
    }
}
