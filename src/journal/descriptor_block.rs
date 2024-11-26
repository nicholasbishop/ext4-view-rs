// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::error::{CorruptKind, Ext4Error};
use crate::journal::superblock::JournalSuperblock;
use crate::util::{read_u32be, u64_from_hilo};
use alloc::vec::Vec;
use bitflags::bitflags;

pub(super) fn is_descriptor_block_checksum_valid(
    superblock: &JournalSuperblock,
    block: &[u8],
) -> bool {
    // OK to unwrap: minimum block length is 1024.
    let checksum_offset = block.len().checked_sub(4).unwrap();
    let expected_checksum = read_u32be(block, checksum_offset);
    let mut checksum = Checksum::new();
    checksum.update(superblock.uuid.as_bytes());
    checksum.update(&block[..checksum_offset]);
    checksum.update_u32_be(0);

    checksum.finalize() == expected_checksum
}

// TODO: the kernel docs for this are a mess
#[derive(Debug)]
pub(super) struct JournalDescriptorBlockTag {
    pub(super) block_number: u64,
    pub(super) flags: JournalDescriptorBlockTagFlags,
    pub(super) checksum: u32,
    #[expect(dead_code)] // TODO
    uuid: [u8; 16],
}

impl JournalDescriptorBlockTag {
    pub(super) fn read_bytes(bytes: &[u8]) -> (Self, usize) {
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
    pub(super) fn read_bytes_to_vec(
        mut bytes: &[u8],
    ) -> Result<Vec<Self>, Ext4Error> {
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

        Err(CorruptKind::JournalDescriptorBlockMissingLastTag.into())
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
