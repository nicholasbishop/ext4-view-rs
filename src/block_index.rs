// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::extent::Extent;
use crate::util::usize_from_u32;
use core::ops::{Add, Sub};

// TODO: make var names consistent, e.g. rename relative_index and such

/// Absolute block index within the filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct FsBlockIndex(u64);

impl FsBlockIndex {
    /// Convert to a byte index using the provided `block_size`.
    #[inline]
    pub(crate) fn to_byte(self, block_size: u32) -> u64 {
        self.0 * u64::from(block_size)
    }

    /// Returns whether the block index is zero.
    #[inline]
    pub(crate) fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Add<u64> for FsBlockIndex {
    type Output = Self;

    #[inline]
    fn add(self, v: u64) -> Self {
        Self(self.0 + v)
    }
}

impl From<u64> for FsBlockIndex {
    #[inline]
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<u32> for FsBlockIndex {
    #[inline]
    fn from(v: u32) -> Self {
        Self(u64::from(v))
    }
}

/// Block index relative to the start of the file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct FileBlockIndex(u32);

impl FileBlockIndex {
    // TODO: consider 'within_range' if this is needed for anything
    // other than extents
    pub(crate) fn is_within_extent(self, extent: &Extent) -> bool {
        let start = extent.block_within_file;
        let end = start.0 + u32::from(extent.num_blocks);

        self.0 >= start.0 && self.0 < end
    }

    /// Get the index as a [`usize`].
    #[inline]
    pub(crate) fn to_usize(self) -> usize {
        usize_from_u32(self.0)
    }
}

// impl Sub<Self> for FileBlockIndex {
//     type Output = u32;

//     #[inline]
//     fn sub(self, v: Self) -> u32 {
//         self.0.checked_sub(v.0).unwrap()
//     }
// }

impl From<u32> for FileBlockIndex {
    #[inline]
    fn from(v: u32) -> Self {
        Self(v)
    }
}
