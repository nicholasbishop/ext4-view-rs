// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::util::{read_u32be, u64_from_hilo};
use crate::{Corrupt, Ext4, Ext4Error, Incompatible};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::bitflags;

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
        const JOURNAL_SUPERBLOCK_SIZE: usize = 1024;
        const JOURNAL_CHECKSUM_TYPE_CRC32C: u8 = 4;

        let Some(journal_inode) = fs.0.superblock.journal_inode else {
            // Return an empty journal if this filesystem does not have
            // a journal.
            return Ok(Self::empty());
        };

        // Get an iterator over the journal's block indices.
        let journal_inode = Inode::read(fs, journal_inode)?;
        let mut journal_block_iter =
            FileBlocks::new(fs.clone(), &journal_inode)?;

        // Read the first 1024 bytes of the first block. This is the
        // journal's superblock.
        let block_index =
            journal_block_iter.next().ok_or(Corrupt::JournalSize)??;
        let mut block = vec![0; JOURNAL_SUPERBLOCK_SIZE];
        fs.read_from_block(block_index, 0, &mut block)?;

        // Check superblock type.
        let header = JournalBlockHeader::read_bytes(&block)?;
        if header.block_type != JournalBlockType::SUPERBLOCK_V2 {
            return Err(Incompatible::JournalSuperblockType(
                header.block_type.0,
            )
            .into());
        }

        let s_blocksize = read_u32be(&block, 0xc);
        let s_sequence = read_u32be(&block, 0x18);
        let s_start = read_u32be(&block, 0x1c);
        let s_feature_incompat = read_u32be(&block, 0x28);
        let s_checksum_type = block[0x50];
        let s_checksum = read_u32be(&block, 0xfc);

        // Validate the superblock checksum.
        if s_checksum_type == JOURNAL_CHECKSUM_TYPE_CRC32C {
            let mut checksum = Checksum::new();
            checksum.update(&block[..0xfc]);
            checksum.update_u32_le(0);
            checksum.update(&block[0xfc + 4..1024]);
            if checksum.finalize() != s_checksum {
                return Err(Corrupt::JournalSuperblockChecksum.into());
            }
        } else {
            return Err(
                Incompatible::JournalChecksumType(s_checksum_type).into()
            );
        }

        // Check that required features are present, and that no other
        // features are present.
        let required_incompat_features = JournalIncompatibleFeatures::IS_64BIT
            | JournalIncompatibleFeatures::CHECKSUM_V3;
        let incompat_features =
            JournalIncompatibleFeatures::from_bits_retain(s_feature_incompat);
        if incompat_features != required_incompat_features {
            return Err(Incompatible::JournalIncompatibleFeatures(
                s_feature_incompat,
            )
            .into());
        }

        // Ensure the journal block size matches the rest of the
        // filesystem.
        let block_size = fs.0.superblock.block_size;
        if s_blocksize != block_size {
            return Err(Corrupt::JournalBlockSize.into());
        }

        // Resize the block (which previously contained just the
        // superblock) to the full block size in preparation for reading
        // journal blocks.
        block.resize(block_size.to_usize(), 0u8);

        // Skip forward to the start block. Iteration starts at one here
        // because we've already read block zero, the superblock.
        for _ in 1..s_start {
            journal_block_iter
                .next()
                .ok_or(Corrupt::JournalTruncated)??;
        }

        let mut is_first_commit = true;

        let mut block_map = BTreeMap::new();
        let mut uncommitted_block_map = BTreeMap::new();
        while let Some(block_index) = journal_block_iter.next() {
            let block_index = block_index?;

            fs.read_from_block(block_index, 0, &mut block)?;

            // TODO: validate checksums.

            let h_magic = read_u32be(&block, 0x0);
            if h_magic != 0xc03b3998 {
                // No magic.
                // dbg!("no magic");
                break;
            }

            let header = JournalBlockHeader::read_bytes(&block)?;

            if header.block_type == JournalBlockType::DESCRIPTOR {
                let tags =
                    JournalDescriptorBlockTag::read_bytes_to_vec(&block[12..])
                        .unwrap();

                for tag in &tags {
                    let block_index = journal_block_iter
                        .next()
                        .ok_or(Corrupt::JournalTruncated)??;

                    uncommitted_block_map.insert(tag.block_number, block_index);
                }
            } else if header.block_type == JournalBlockType::COMMIT {
                // TODO: do other stuff with the commit block.

                if is_first_commit {
                    is_first_commit = false;
                    if header.sequence != s_sequence {
                        return Err(Corrupt::JournalSequence.into());
                    }
                }

                // Move the entries from `uncommitted_block_map` to `block_map`.
                block_map.extend(uncommitted_block_map.iter());
                uncommitted_block_map.clear();
            } else {
                todo!()
            }
        }

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JournalBlockType(u32);

impl JournalBlockType {
    const DESCRIPTOR: Self = Self(1);
    const COMMIT: Self = Self(2);
    const SUPERBLOCK_V2: Self = Self(4);
}

#[derive(Debug)]
struct JournalBlockHeader {
    block_type: JournalBlockType,
    sequence: u32,
}

impl JournalBlockHeader {
    fn read_bytes(bytes: &[u8]) -> Result<Self, Ext4Error> {
        assert!(bytes.len() >= 12);

        let h_magic = read_u32be(bytes, 0x0);
        let h_blocktype = read_u32be(bytes, 0x4);
        let h_sequence = read_u32be(bytes, 0x8);

        // Check journal magic.
        if h_magic != 0xc03b3998 {
            return Err(Corrupt::JournalMagic.into());
        }

        let block_type = JournalBlockType(h_blocktype);

        Ok(Self {
            block_type,
            sequence: h_sequence,
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct JournalIncompatibleFeatures: u32 {
        const BLOCK_REVOCATIONS = 0x1;
        const IS_64BIT = 0x2;
        const ASYNC_COMMITS = 0x4;
        const CHECKSUM_V2 = 0x8;
        const CHECKSUM_V3 = 0x10;
        const FAST_COMMITS = 0x20;
    }
}

// TODO: the kernel docs for this are a mess
#[derive(Debug)]
struct JournalDescriptorBlockTag {
    block_number: u64,
    flags: JournalDescriptorBlockTagFlags,
    #[expect(dead_code)] // TODO
    checksum: u32,
    #[expect(dead_code)] // TODO
    uuid: [u8; 16],
}

impl JournalDescriptorBlockTag {
    fn read_bytes(bytes: &[u8]) -> (Self, usize) {
        // TODO: for now assuming the `incompat_features` assert above.

        let t_blocknr = read_u32be(bytes, 0);
        let t_flags = read_u32be(bytes, 4);
        let t_blocknr_high = read_u32be(bytes, 8);
        let t_checksum = read_u32be(bytes, 12);

        let flags = JournalDescriptorBlockTagFlags::from_bits_retain(t_flags);
        let mut size: usize = 16;

        let mut uuid = [0; 16];
        if !flags.contains(JournalDescriptorBlockTagFlags::UUID_OMITTED) {
            // OK to unwrap: length is 16.
            uuid = bytes[16..32].try_into().unwrap();
            // TODO: unwrap
            size = size.checked_add(16).unwrap();
        }

        (
            Self {
                block_number: u64_from_hilo(t_blocknr_high, t_blocknr),
                flags,
                checksum: t_checksum,
                uuid,
            },
            size,
        )
    }

    // TODO: this could be an iterator instead of allocating.
    fn read_bytes_to_vec(mut bytes: &[u8]) -> Result<Vec<Self>, Ext4Error> {
        let mut v = Vec::new();

        while !bytes.is_empty() {
            let (tag, size) = Self::read_bytes(bytes);
            let is_end =
                tag.flags.contains(JournalDescriptorBlockTagFlags::LAST_TAG);
            v.push(tag);

            if is_end {
                return Ok(v);
            }

            bytes = &bytes[size..];
        }

        Err(Corrupt::JournalDescriptorBlockMissingLastTag.into())
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct JournalDescriptorBlockTagFlags: u32 {
        const ESCAPED = 0x1;
        const UUID_OMITTED = 0x2;
        const DELETED = 0x4;
        const LAST_TAG = 0x8;
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
