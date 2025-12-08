// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::test_util::load_compressed_filesystem;

/// Test reading an inode from a filesystem with the minimum inode size
/// of 128 bytes.
#[test]
fn test_read_small_inode() {
    let fs = load_compressed_filesystem("test_disk_ext3.bin.zst");
    let mut dir_iter = fs.read_dir("/").unwrap();
    let entry = dir_iter.next().unwrap().unwrap();
    assert_eq!(entry.file_name(), ".");
}
