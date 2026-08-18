// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

/// A file timestamp, as stored in an inode.
///
/// Timestamps are relative to the Unix epoch (1970-01-01 00:00:00 UTC), and
/// may be negative for earlier times.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Timestamp {
    seconds: i64,
    nanoseconds: u32,
}

impl Timestamp {
    /// Create a timestamp from a second count and a nanosecond offset.
    ///
    /// Useful for comparing against a known time, and for exercising the edges of
    /// a narrower time type downstream (NFSv3, for example, carries unsigned
    /// 32-bit seconds and needs to clamp).
    #[must_use]
    pub fn new(seconds: i64, nanoseconds: u32) -> Self {
        Self {
            seconds,
            nanoseconds,
        }
    }

    /// Decode a timestamp from its on-disk representation.
    ///
    /// `seconds_field` is one of the inode's 32-bit timestamp fields, which
    /// holds a *signed* count of seconds since the Unix epoch.
    ///
    /// `extra_field` is the corresponding `*_extra` field, which only exists
    /// in inodes larger than 128 bytes; pass zero when it is absent. Its low
    /// two bits extend the seconds field past 2038, and its upper 30 bits
    /// hold the nanoseconds.
    pub(crate) fn from_inode_fields(
        seconds_field: u32,
        extra_field: u32,
    ) -> Self {
        // Reinterpret the field as signed without an `as` cast.
        let base = i32::from_le_bytes(seconds_field.to_le_bytes());

        // The low two bits are an unsigned extension of the seconds field.
        let epoch_extension = i64::from(extra_field & 0x3) << 32;

        Self {
            // Both operands are bounded (a 32-bit base and a 34-bit
            // extension), so this cannot actually saturate.
            seconds: i64::from(base).saturating_add(epoch_extension),
            nanoseconds: extra_field >> 2,
        }
    }

    /// Seconds since the Unix epoch. Negative for times before 1970.
    #[must_use]
    pub fn seconds(self) -> i64 {
        self.seconds
    }

    /// Nanoseconds within the second.
    ///
    /// This is zero for inodes that are too small to have the extra
    /// timestamp fields (128-byte inodes), since they only store
    /// second-granularity times.
    ///
    /// The on-disk field is 30 bits wide, so a corrupt file system can
    /// produce a value larger than `999_999_999`. The value is returned as
    /// stored rather than clamped, so callers converting to another time
    /// type should handle that case.
    #[must_use]
    pub fn nanoseconds(self) -> u32 {
        self.nanoseconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let timestamp = Timestamp::new(-5, 42);
        assert_eq!(timestamp.seconds(), -5);
        assert_eq!(timestamp.nanoseconds(), 42);
    }

    #[test]
    fn test_no_extra_field() {
        // 128-byte inodes have no `*_extra` field, so nanoseconds are zero.
        let timestamp = Timestamp::from_inode_fields(1_765_162_262, 0);
        assert_eq!(timestamp.seconds(), 1_765_162_262);
        assert_eq!(timestamp.nanoseconds(), 0);
    }

    #[test]
    fn test_nanoseconds() {
        // The upper 30 bits of the extra field hold nanoseconds.
        let timestamp = Timestamp::from_inode_fields(0x6788_7c01, 0x7c71_5ecc);
        assert_eq!(timestamp.seconds(), 1_736_997_889);
        assert_eq!(timestamp.nanoseconds(), 521_951_155);
    }

    #[test]
    fn test_epoch_extension() {
        // The low two bits of the extra field extend the seconds field, so
        // that times past 2038 are representable.
        let base = 0u32;
        assert_eq!(Timestamp::from_inode_fields(base, 0).seconds(), 0);
        assert_eq!(Timestamp::from_inode_fields(base, 1).seconds(), 1 << 32);
        assert_eq!(Timestamp::from_inode_fields(base, 2).seconds(), 2i64 << 32);
        assert_eq!(Timestamp::from_inode_fields(base, 3).seconds(), 3i64 << 32);

        // A negative base combined with an epoch extension: this is how
        // dates after 2038 are stored, since the base wraps to negative.
        let past_2038 = Timestamp::from_inode_fields(0x8000_0000, 1);
        assert_eq!(past_2038.seconds(), i64::from(i32::MIN) + (1 << 32));
        assert_eq!(past_2038.seconds(), 2_147_483_648);
    }

    #[test]
    fn test_negative_seconds() {
        // Times before 1970 are stored as a negative base with no epoch
        // extension bits.
        let timestamp = Timestamp::from_inode_fields(0xffff_ffff, 0);
        assert_eq!(timestamp.seconds(), -1);
    }

    #[test]
    fn test_out_of_range_nanoseconds() {
        // A corrupt file system can store more than one second worth of
        // nanoseconds. The value is passed through as stored.
        let timestamp = Timestamp::from_inode_fields(0, u32::MAX);
        assert_eq!(timestamp.nanoseconds(), u32::MAX >> 2);
        assert!(timestamp.nanoseconds() > 999_999_999);
    }
}
