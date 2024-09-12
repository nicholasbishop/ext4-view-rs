// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ext4_view::Ext4;

fn load_ext2() -> Ext4 {
    let output = std::process::Command::new("zstd")
        .args([
            "--decompress",
            // Write to stdout and don't delete the input file.
            "--stdout",
            "test_data/test_disk_ext2.bin.zst",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    Ext4::load(Box::new(output.stdout)).unwrap()
}

// This function is duplicated in `/xtask/src/main.rs`.
fn gen_big_file(num_blocks: u32) -> Vec<u8> {
    let mut file = Vec::new();
    let block_size = 1024;
    for i in 0..num_blocks {
        let mut block = vec![0; block_size];
        let i_le = i.to_le_bytes();
        block[..4].copy_from_slice(&i_le);
        block[block_size - 4..].copy_from_slice(&i_le);
        file.extend(block);
    }
    file
}

#[test]
fn test_read_small_file() {
    let fs = load_ext2();
    assert_eq!(fs.read("/small_file").unwrap(), b"hello, world!");
}

#[test]
fn test_read_big_file() {
    let fs = load_ext2();
    let num_blocks = 12 + 256 + (256 * 256) + (256 * 16);
    assert_eq!(fs.read("/big_file").unwrap(), gen_big_file(num_blocks));
}
