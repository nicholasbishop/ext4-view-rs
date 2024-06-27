// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#[test]
fn test_read_dir_debug() {
    let fs = crate::load_test_disk1();
    let read_dir = fs.read_dir("/big_dir").unwrap();
    assert_eq!(format!("{:?}", read_dir), r#"ReadDir("/big_dir")"#);
}
