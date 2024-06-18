// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::format::format_bytes_debug;
use crate::inode::InodeIndex;
use core::fmt::{self, Debug, Formatter};
use core::str::Utf8Error;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DirEntryName<'a>(pub(crate) &'a [u8]);

impl<'a> DirEntryName<'a> {
    /// Maximum length of a `DirEntryName`.
    pub const MAX_LEN: usize = 255;

    /// Convert to a `&str` if the name is valid UTF-8.
    #[inline]
    pub fn as_str(&self) -> Result<&'a str, Utf8Error> {
        core::str::from_utf8(self.0)
    }
}

impl<'a> Debug for DirEntryName<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.0, f)
    }
}

#[derive(Clone, Eq, Ord, PartialOrd)]
struct DirEntryNameBuf {
    data: [u8; DirEntryName::MAX_LEN],
    len: u8,
}

impl DirEntryNameBuf {
    #[inline]
    #[must_use]
    fn as_bytes(&self) -> &[u8] {
        &self.data[..usize::from(self.len)]
    }
}

impl Debug for DirEntryNameBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        format_bytes_debug(self.as_bytes(), f)
    }
}

// Manual implementation of `PartialEq` because we don't want to compare
// the entire `data` array, only up to `len`.
impl PartialEq<DirEntryNameBuf> for DirEntryNameBuf {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

/// Directory entry.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DirEntry {
    /// Number of the inode that this entry points to.
    inode: InodeIndex,

    /// Raw name of the entry.
    name: DirEntryNameBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_entry_debug() {
        let src = "abc😁\n".as_bytes();
        let expected = "abc😁\\n"; // Note the escaped slash.
        assert_eq!(format!("{:?}", DirEntryName(src)), expected);

        let mut src_vec = src.to_vec();
        src_vec.resize(255, 0);
        assert_eq!(
            format!(
                "{:?}",
                DirEntryNameBuf {
                    data: src_vec.try_into().unwrap(),
                    len: src.len().try_into().unwrap(),
                }
            ),
            expected
        );
    }
}
