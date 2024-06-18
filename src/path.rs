// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::format::format_bytes_debug;
use alloc::vec::Vec;
use core::fmt::{self, Debug, Formatter};

/// Reference path type.
///
/// Paths are mostly arbitrary sequences of bytes, with two restrictions:
/// * The path cannot contain any null bytes.
/// * Each component of the path must be no longer than 255 bytes.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Path<'a>(
    // Use `&[u8]` rather than `[u8]` so that we don't have to use any
    // unsafe code. Unfortunately that means we can't impl `Deref` to
    // convert from `PathBuf` to `Path`.
    &'a [u8],
);

impl<'a> Debug for Path<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.0, f)
    }
}

/// Owned path type.
///
/// Paths are mostly arbitrary sequences of bytes, with two restrictions:
/// * The path cannot contain any null bytes.
/// * Each component of the path must be no longer than 255 bytes.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PathBuf(Vec<u8>);

impl PathBuf {
    /// Borrow as a `Path`.
    pub fn as_path(&self) -> Path {
        Path(&self.0)
    }
}

impl Debug for PathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.as_path().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_debug() {
        let src = "abc😁\n".as_bytes();
        let expected = "abc😁\\n"; // Note the escaped slash.
        assert_eq!(format!("{:?}", Path(src)), expected);
        assert_eq!(format!("{:?}", PathBuf(src.to_vec())), expected);
    }
}
