// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// TODO(nicholasbishop): Temporarily allow dead code to allow for
// smaller PRs.
#![allow(dead_code)]

extern crate alloc;

mod block_group;
mod checksum;
mod error;
mod features;
mod superblock;
mod util;
