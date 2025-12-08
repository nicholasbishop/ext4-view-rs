// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::test_util::load_compressed_filesystem;
use ext4_view::{Ext4, Path};

pub fn load_ext3() -> Ext4 {
    load_compressed_filesystem("test_disk_ext3.bin.zst")
}

/// Test reading an inode from a filesystem with the minimum inode size
/// of 128 bytes.
#[test]
fn test_read_small_inode() {
    let fs = load_ext3();
    let mut dir_iter = fs.read_dir("/").unwrap();
    let entry = dir_iter.next().unwrap().unwrap();
    assert_eq!(entry.file_name(), ".");
}

/// Test reading files from an htree directory that uses TEA hashes.
#[test]
fn test_tea_htree() {
    let fs = load_ext3();

    let medium_dir = Path::new("/medium_dir");
    for i in 0..1_000 {
        let i = i.to_string();
        assert_eq!(fs.read_to_string(&medium_dir.join(&i)).unwrap(), i);
    }
}
