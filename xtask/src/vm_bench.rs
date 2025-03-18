// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::Result;
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};

pub fn run_vm_bench() -> Result<()> {
    let prebuilt = Prebuilt::fetch(Source::LATEST, "target/ovmf")
        .expect("failed to update prebuilt");

    let _code = prebuilt.get_file(Arch::X64, FileType::Code);
    let _vars = prebuilt.get_file(Arch::X64, FileType::Vars);

    todo!();
}
