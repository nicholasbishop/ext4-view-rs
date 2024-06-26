// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use ext4_view::Ext4;

#[test]
fn test_read() {
    let data = include_bytes!("../../test_data/test_disk1.bin");
    let fs = Ext4::load(Box::new(data.to_vec())).unwrap();

    // Empty file.
    assert_eq!(fs.read("/empty_file").unwrap(), []);

    // Small file.
    assert_eq!(fs.read("/small_file").unwrap(), b"hello, world!");

    // File with holes.
    let mut expected = vec![];
    for i in 0..5 {
        expected.extend(vec![0xa5; 4096]);
        if i != 4 {
            expected.extend(vec![0; 8192]);
        }
    }
    assert_eq!(fs.read("/holes").unwrap(), expected);

    // Errors.
    assert!(fs.read("not_absolute").is_err());
    assert!(fs.read("/does_not_exist").is_err());
}
