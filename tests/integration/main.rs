// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod ext2;
mod ext3;
mod ext4;
mod file;
mod label;
mod path;
mod uuid;

use ext4_view::Ext4;

mod test_util {
    include!("../../src/test_util.rs");
}

/// Get the expected data for the "/holes" file.
///
/// Should match `create_file_with_holes` in xtask.
fn expected_holes_data() -> Vec<u8> {
    let block_size = 1024;

    let hole_block = vec![0; block_size];

    let mut expected = Vec::new();
    expected.extend(&hole_block);
    expected.extend(&hole_block);
    expected.extend(vec![0xa1; block_size]);
    expected.extend(vec![0xa2; block_size]);
    expected.extend(&hole_block);
    expected.extend(&hole_block);
    expected.extend(vec![0xa3; block_size]);
    expected.extend(vec![0xa4; block_size]);
    expected.extend(&hole_block);
    expected.extend(&hole_block);

    expected
}
