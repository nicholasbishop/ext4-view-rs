// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod dir;
mod ext4;
mod path;

use ext4_view::Ext4;

fn load_test_disk1() -> Ext4 {
    const DATA: &[u8] = include_bytes!("../../test_data/test_disk1.bin");
    Ext4::load(Box::new(DATA.to_vec())).unwrap()
}
