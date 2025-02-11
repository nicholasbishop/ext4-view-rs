// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::util::{read_u32be, u64_from_hilo};
use bitflags::bitflags;

/// Data block tag within a descriptor block.
///
/// Each descriptor block contains an array of tags, one for each data
/// block following the descriptor block. Each data block will replace a
/// block within the ext4 filesystem. The tag indicates where the data
/// block maps into the filesystem, and provides a checksum for the data
/// block.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct DescriptorBlockTag {
    /// Absolute block index in the filesystem that should be replaced
    /// with the data block associated with this tag.
    pub(super) block_index: u64,

    /// Checksum of the block data.
    ///
    /// Note that this checksum is for the data block associated with
    /// this tag. The data in the tag itself is covered by the
    /// descriptor block checksum.
    pub(super) checksum: u32,

    flags: DescriptorBlockTagFlags,
}

impl DescriptorBlockTag {
    const SIZE_WITHOUT_UUID: usize = 16;
    const SIZE_WITH_UUID: usize = 32;

    /// Read a tag from `bytes`.
    ///
    /// Returns `None` if there are not enough bytes to read the tag.
    fn read_bytes(bytes: &[u8]) -> Option<Self> {
        // Note: the tag format depends on feature flags in the journal
        // superblock. The code in this function is only correct if the
        // `CHECKSUM_V3` feature is enabled (this is checked when
        // loading the superblock).

        if bytes.len() < Self::SIZE_WITHOUT_UUID {
            return None;
        }

        let t_blocknr = read_u32be(bytes, 0);
        let t_flags = read_u32be(bytes, 4);
        let t_blocknr_high = read_u32be(bytes, 8);
        let t_checksum = read_u32be(bytes, 12);

        let flags = DescriptorBlockTagFlags::from_bits_retain(t_flags);

        if !flags.contains(DescriptorBlockTagFlags::UUID_OMITTED)
            && bytes.len() < Self::SIZE_WITH_UUID
        {
            return None;
        }

        Some(Self {
            block_index: u64_from_hilo(t_blocknr_high, t_blocknr),
            flags,
            checksum: t_checksum,
        })
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    struct DescriptorBlockTagFlags: u32 {
        const ESCAPED = 0x1;
        const UUID_OMITTED = 0x2;
        const DELETED = 0x4;
        const LAST_TAG = 0x8;
    }
}
