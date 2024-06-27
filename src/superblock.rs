// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::checksum::Checksum;
use crate::error::{Corrupt, Ext4Error, Incompatible};
use crate::features::{IncompatibleFeatures, ReadOnlyCompatibleFeatures};
use crate::util::{read_u16le, read_u32le, u64_from_hilo};

/// Information about the filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Superblock {
    pub(crate) block_size: u32,
    pub(crate) blocks_count: u64,
    pub(crate) inode_size: u16,
    pub(crate) inodes_per_block_group: u32,
    pub(crate) block_group_descriptor_size: u16,
    pub(crate) num_block_groups: u32,
    pub(crate) incompatible_features: IncompatibleFeatures,
    pub(crate) read_only_compatible_features: ReadOnlyCompatibleFeatures,
    pub(crate) checksum_seed: u32,
    pub(crate) htree_hash_seed: [u32; 4],
}

impl Superblock {
    /// Size (in bytes) of the superblock on disk.
    pub(crate) const SIZE_IN_BYTES_ON_DISK: usize = 1024;

    /// Construct `Superblock` from bytes.
    ///
    /// # Panics
    ///
    /// Panics if the length of `bytes` is less than
    /// [`Self::SIZE_IN_BYTES_ON_DISK`].
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, Ext4Error> {
        assert!(bytes.len() >= Self::SIZE_IN_BYTES_ON_DISK);

        // OK to unwrap: already checked the length.
        let s_blocks_count_lo = read_u32le(bytes, 0x4);
        let s_first_data_block = read_u32le(bytes, 0x14);
        let s_log_block_size = read_u32le(bytes, 0x18);
        let s_blocks_per_group = read_u32le(bytes, 0x20);
        let s_inodes_per_group = read_u32le(bytes, 0x28);
        let s_magic = read_u16le(bytes, 0x38);
        let s_inode_size = read_u16le(bytes, 0x58);
        let s_feature_incompat = read_u32le(bytes, 0x60);
        let s_feature_ro_compat = read_u32le(bytes, 0x64);
        let s_uuid = &bytes[0x68..0x68 + 16];
        const S_HASH_SEED_OFFSET: usize = 0xec;
        let s_hash_seed = [
            read_u32le(bytes, S_HASH_SEED_OFFSET),
            read_u32le(bytes, S_HASH_SEED_OFFSET + 4),
            read_u32le(bytes, S_HASH_SEED_OFFSET + 8),
            read_u32le(bytes, S_HASH_SEED_OFFSET + 12),
        ];
        let s_desc_size = read_u16le(bytes, 0xfe);
        let s_blocks_count_hi = read_u32le(bytes, 0x150);
        let s_checksum_seed = read_u32le(bytes, 0x270);
        const S_CHECKSUM_OFFSET: usize = 0x3fc;
        let s_checksum = read_u32le(bytes, S_CHECKSUM_OFFSET);

        let blocks_count = u64_from_hilo(s_blocks_count_hi, s_blocks_count_lo);

        let block_size = 2u32.pow(10 + s_log_block_size);

        if s_magic != 0xef53 {
            return Err(Ext4Error::Corrupt(Corrupt::SuperblockMagic));
        }

        let incompatible_features = check_incompat_features(s_feature_incompat)
            .map_err(Ext4Error::Incompatible)?;
        let read_only_compatible_features =
            ReadOnlyCompatibleFeatures::from_bits_retain(s_feature_ro_compat);

        // s_first_data_block is usually 1 if the block size is 1KiB,
        // and otherwise its usually 0.
        let num_data_blocks = blocks_count - u64::from(s_first_data_block);
        // Use div_ceil to round up in case `num_data_blocks` isn't an
        // even multiple of `s_blocks_per_group`. (Consider for example
        // `num_data_blocks = 3` and `s_blocks_per_group = 4`; that is
        // one block group, but regular division would calculate zero
        // instead of one.)
        let num_block_groups = u32::try_from(
            num_data_blocks.div_ceil(u64::from(s_blocks_per_group)),
        )
        .map_err(|_| Ext4Error::Corrupt(Corrupt::TooManyBlockGroups))?;

        // Validate the superblock checksum.
        if read_only_compatible_features
            .contains(ReadOnlyCompatibleFeatures::METADATA_CHECKSUMS)
        {
            let mut checksum = Checksum::new();
            checksum.update(&bytes[..S_CHECKSUM_OFFSET]);
            if s_checksum != checksum.finalize() {
                return Err(Ext4Error::Corrupt(Corrupt::SuperblockChecksum));
            }
        }

        let checksum_seed = if incompatible_features
            .contains(IncompatibleFeatures::CHECKSUM_SEED_IN_SUPERBLOCK)
        {
            s_checksum_seed
        } else {
            let mut checksum = Checksum::new();
            checksum.update(s_uuid);
            checksum.finalize()
        };

        Ok(Self {
            block_size,
            blocks_count,
            inode_size: s_inode_size,
            inodes_per_block_group: s_inodes_per_group,
            block_group_descriptor_size: s_desc_size,
            num_block_groups,
            incompatible_features,
            read_only_compatible_features,
            checksum_seed,
            htree_hash_seed: s_hash_seed,
        })
    }
}

fn check_incompat_features(
    s_feature_incompat: u32,
) -> Result<IncompatibleFeatures, Incompatible> {
    let actual = IncompatibleFeatures::from_bits_retain(s_feature_incompat);
    let actual_known =
        IncompatibleFeatures::from_bits_truncate(s_feature_incompat);
    if actual != actual_known {
        return Err(Incompatible::Unknown(actual.difference(actual_known)));
    }

    // TODO: for now, be strict on most incompat features. May be able to
    // relax some of these in the future.
    let required_features = IncompatibleFeatures::FILE_TYPE_IN_DIR_ENTRY
        | IncompatibleFeatures::EXTENTS
        | IncompatibleFeatures::IS_64BIT
        | IncompatibleFeatures::FLEXIBLE_BLOCK_GROUPS;
    let disallowed_features = IncompatibleFeatures::COMPRESSION
        | IncompatibleFeatures::RECOVERY
        | IncompatibleFeatures::SEPARATE_JOURNAL_DEVICE
        | IncompatibleFeatures::META_BLOCK_GROUPS
        | IncompatibleFeatures::MULTIPLE_MOUNT_PROTECTION
        | IncompatibleFeatures::LARGE_EXTENDED_ATTRIBUTES_IN_INODES
        | IncompatibleFeatures::DATA_IN_DIR_ENTRY
        | IncompatibleFeatures::LARGE_DIRECTORIES
        | IncompatibleFeatures::DATA_IN_INODE
        | IncompatibleFeatures::ENCRYPTED_INODES;

    let present_required = actual & required_features;
    if present_required != required_features {
        return Err(Incompatible::Missing(
            required_features.difference(present_required),
        ));
    }

    let present_disallowed = actual & disallowed_features;
    if !present_disallowed.is_empty() {
        return Err(Incompatible::Incompatible(present_disallowed));
    }

    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_superblock() {
        let data = include_bytes!("../test_data/raw_superblock.bin");
        let sb = Superblock::from_bytes(data).unwrap();
        assert_eq!(
            sb,
            Superblock {
                block_size: 1024,
                blocks_count: 128,
                inode_size: 256,
                inodes_per_block_group: 16,
                block_group_descriptor_size: 64,
                num_block_groups: 1,
                incompatible_features:
                    IncompatibleFeatures::FILE_TYPE_IN_DIR_ENTRY
                        | IncompatibleFeatures::EXTENTS
                        | IncompatibleFeatures::IS_64BIT
                        | IncompatibleFeatures::FLEXIBLE_BLOCK_GROUPS
                        | IncompatibleFeatures::CHECKSUM_SEED_IN_SUPERBLOCK,
                read_only_compatible_features:
                    ReadOnlyCompatibleFeatures::SPARSE_SUPERBLOCKS
                        | ReadOnlyCompatibleFeatures::LARGE_FILES
                        | ReadOnlyCompatibleFeatures::HUGE_FILES
                        | ReadOnlyCompatibleFeatures::LARGE_DIRECTORIES
                        | ReadOnlyCompatibleFeatures::LARGE_INODES
                        | ReadOnlyCompatibleFeatures::METADATA_CHECKSUMS,
                checksum_seed: 0xfd3cc0be,
                htree_hash_seed: [
                    0xbb071441, 0x7746982f, 0x6007bb8f, 0xb61a9b7
                ],
            }
        );
    }

    /// Test that an error is returned if an unknown incompatible
    /// feature bit is set. Test that the error value contains only the
    /// unknown bits.
    #[test]
    fn test_unknown_incompat_flags() {
        let mut data =
            include_bytes!("../test_data/raw_superblock.bin").to_vec();
        data[0x62] |= 0x02;
        assert_eq!(
            *Superblock::from_bytes(&data)
                .unwrap_err()
                .as_incompatible()
                .unwrap(),
            Incompatible::Unknown(IncompatibleFeatures::from_bits_retain(
                0x2_0000
            ))
        );
    }

    #[test]
    fn test_check_incompat_features() {
        let required = (IncompatibleFeatures::FILE_TYPE_IN_DIR_ENTRY
            | IncompatibleFeatures::EXTENTS
            | IncompatibleFeatures::IS_64BIT
            | IncompatibleFeatures::FLEXIBLE_BLOCK_GROUPS
            | IncompatibleFeatures::CHECKSUM_SEED_IN_SUPERBLOCK)
            .bits();

        // Success.
        assert!(check_incompat_features(required).is_ok());

        // Unknown incompatible bit is an error.
        assert_eq!(
            check_incompat_features(required | 0x2_0000).unwrap_err(),
            Incompatible::Unknown(IncompatibleFeatures::from_bits_retain(
                0x2_0000
            ))
        );

        assert_eq!(
            check_incompat_features(
                required & (!IncompatibleFeatures::IS_64BIT.bits())
            )
            .unwrap_err(),
            Incompatible::Missing(IncompatibleFeatures::IS_64BIT)
        );

        assert_eq!(
            check_incompat_features(
                required | IncompatibleFeatures::RECOVERY.bits()
            )
            .unwrap_err(),
            Incompatible::Incompatible(IncompatibleFeatures::RECOVERY)
        );
    }
}
