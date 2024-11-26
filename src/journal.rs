// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// TODO
#![expect(dead_code, clippy::arithmetic_side_effects, clippy::as_conversions)]

use crate::inode::Inode;
use crate::iters::file_blocks::FileBlocks;
use crate::util::{read_u32be, u64_from_hilo};
use crate::{Corrupt, Ext4, Ext4Error};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use bitflags::bitflags;

#[derive(Debug)]
pub(crate) struct Journal {
    // TODO: not sure if we want to store this here, or just redirect
    // the read?
    blocks: BTreeMap<u64, u64>,
}

impl Journal {
    pub(crate) fn empty() -> Self {
        Self {
            blocks: BTreeMap::new(),
        }
    }

    pub(crate) fn load(fs: &Ext4) -> Result<Self, Ext4Error> {
        // Note: ext4 is all little-endian, except for the journal,
        // which is all big-endian. 😬

        let Some(journal_inode) = fs.0.superblock.journal_inode else {
            // Return an empty journal if this filesystem does not have
            // a journal.
            return Ok(Self::empty());
        };

        let journal_inode = Inode::read(fs, journal_inode)?;

        let mut journal_block_iter =
            FileBlocks::new(fs.clone(), &journal_inode)?;
        let block_index =
            journal_block_iter.next().ok_or(Corrupt::JournalSize)??;

        let block_size = fs.0.superblock.block_size;
        let mut block = vec![0; block_size.to_usize()];
        fs.read_bytes(block_index * block_size.to_u64(), &mut block)?;

        // The journal superblock is 1024 bytes.
        if block.len() < 1024 {
            return Err(Corrupt::JournalSize.into());
        }

        let header = JournalBlockHeader::read_bytes(&block)?;

        // Check superblock type.
        if ![
            JournalBlockType::SuperblockV1,
            JournalBlockType::SuperblockV2,
        ]
        .contains(&header.block_type)
        {
            return Err(Corrupt::JournalSuperblockType(
                header.block_type as u32,
            )
            .into());
        }

        // TODO: return not-supported for v1.
        assert_eq!(header.block_type, JournalBlockType::SuperblockV2);

        let s_blocksize = read_u32be(&block, 0xc);
        let _s_maxlen = read_u32be(&block, 0x10);
        // TODO: what's the difference between first and start?
        let _s_first = read_u32be(&block, 0x14);
        let _s_sequence = read_u32be(&block, 0x18);
        let s_start = read_u32be(&block, 0x1c);
        let s_feature_compat = read_u32be(&block, 0x24);
        let s_feature_incompat = read_u32be(&block, 0x28);

        // TODO: check features.
        // TODO: checksum type

        // TODO
        assert_eq!(s_blocksize, block_size);

        let compat_features =
            JournalCompatibleFeatures::from_bits_retain(s_feature_compat);
        let incompat_features =
            JournalIncompatibleFeatures::from_bits_retain(s_feature_incompat);

        // TODO
        assert_eq!(compat_features, JournalCompatibleFeatures::empty());
        assert!(
            incompat_features.contains(JournalIncompatibleFeatures::IS_64BIT)
        );
        //JournalIncompatibleFeatures::BLOCK_REVOCATIONS |
        //| JournalIncompatibleFeatures::CHECKSUM_V3

        let mut blocks = BTreeMap::new();
        // TODO... minus 1 because already read the journal superblock
        for _ in 0..(s_start - 1) {
            // TODO: unwrap
            journal_block_iter.next().unwrap()?;
        }
        while let Some(block_index) = journal_block_iter.next() {
            let block_index = block_index?;

            // TODO: not all blocks need to be read...
            fs.read_bytes(block_index * block_size.to_u64(), &mut block)?;

            // TODO: validate checksums.

            let h_magic = read_u32be(&block, 0x0);
            if h_magic != 0xc03b3998 {
                // No magic.
                // dbg!("no magic");
                break;
            }

            let header = JournalBlockHeader::read_bytes(&block)?;

            if header.block_type == JournalBlockType::Descriptor {
                let tags =
                    JournalDescriptorBlockTag::read_bytes_to_vec(&block[12..])
                        .unwrap();

                // TODO: are these blocks the size of the filesystem or
                // of the journal? Or always the same?

                for tag in &tags {
                    // TODO: unwrap
                    let block_index = journal_block_iter.next().unwrap()?;

                    blocks.insert(tag.block_number, block_index);
                }
            } else if header.block_type == JournalBlockType::Commit {
                // TODO: do stuff with the commit block
            } else {
                todo!()
            }
        }

        Ok(Self { blocks })
    }

    pub(crate) fn map_block_index(&self, block_index: u64) -> u64 {
        *self.blocks.get(&block_index).unwrap_or(&block_index)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JournalBlockType {
    Descriptor,
    Commit,
    SuperblockV1,
    SuperblockV2,
    Revocation,
}

impl JournalBlockType {
    fn new(val: u32) -> Result<Self, Ext4Error> {
        match val {
            1 => Ok(Self::Descriptor),
            2 => Ok(Self::Commit),
            3 => Ok(Self::SuperblockV1),
            4 => Ok(Self::SuperblockV2),
            5 => Ok(Self::Revocation),
            _ => Err(Corrupt::JournalBlockType(val).into()),
        }
    }
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

        // Check superblock type.
        let block_type = JournalBlockType::new(h_blocktype)?;

        Ok(Self {
            block_type,
            sequence: h_sequence,
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct JournalCompatibleFeatures: u32 {
        const CHECKSUMS = 0x1;
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

enum JournalChecksumType {
    Crc32 = 1,
    Md5 = 2,
    Sha1 = 3,
    Crc32c = 4,
}

// TODO: the kernel docs for this are a mess
#[derive(Debug)]
struct JournalDescriptorBlockTag {
    block_number: u64,
    flags: JournalDescriptorBlockTagFlags,
    checksum: u32,
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
        let mut size = 16;

        let mut uuid = [0; 16];
        if !flags.contains(JournalDescriptorBlockTagFlags::UUID_OMITTED) {
            // OK to unwrap: length is 16.
            uuid = bytes[16..32].try_into().unwrap();
            size += 16;
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
        // TODO: return a Corrupt error.
        todo!("missing end tag")
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

// TODO
#[cfg(feature = "std")]
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::rc::Rc;

    // TODO: dedup
    #[cfg(feature = "std")]
    #[cfg(test)]
    fn load_test_fs_with_journal() -> Ext4 {
        // This function executes quickly, so don't bother caching.
        let output = std::process::Command::new("zstd")
            .args([
                "--decompress",
                // Write to stdout and don't delete the input file.
                "--stdout",
                "test_data/test_disk_4k_block_journal.bin.zst",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        Ext4::load(Box::new(output.stdout)).unwrap()
    }

    // TODO
    #[test]
    fn test_journal() {
        let mut fs = load_test_fs_with_journal();

        let block_size = fs.0.superblock.block_size;
        let mut b1 = vec![0; block_size.to_usize()];
        let mut b2 = vec![0; block_size.to_usize()];
        println!("looking for mismatches...");
        for (dst, src) in &fs.0.journal.blocks {
            fs.read_bytes(dst * block_size.to_u64(), &mut b1).unwrap();
            fs.read_bytes(src * block_size.to_u64(), &mut b2).unwrap();
            if b1 != b2 {
                dbg!(dst, src);
            }
            //dbg!(dst, src, b1 == b2);
        }
        println!("done looking");

        let entries = fs
            .read_dir("/")
            .unwrap()
            .map(|e| e.unwrap().file_name().as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        dbg!(entries);
        //todo!();

        let test_dir = "/dir500";

        // With the journal in place, this directory exists.
        assert!(fs.exists(test_dir).unwrap());

        // Clear the journal, and verify that the directory no longer exists.
        Rc::get_mut(&mut fs.0).unwrap().journal.blocks.clear();
        assert!(!fs.exists(test_dir).unwrap());
    }
}
