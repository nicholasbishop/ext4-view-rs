// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use anyhow::{Result, bail};
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run_vm_bench(filesystem: &Path) -> Result<()> {
    build_benchtool()?;
    let esp = build_esp()?;

    let prebuilt = Prebuilt::fetch(Source::LATEST, "target/ovmf")
        .expect("failed to update prebuilt");

    let code = prebuilt.get_file(Arch::X64, FileType::Code);
    let vars = prebuilt.get_file(Arch::X64, FileType::Vars);

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args(["-nodefaults", "--enable-kvm"]);
    cmd.args(["-machine", "q35"]);
    cmd.args(["-m", "1G"]);
    cmd.args(["-serial", "stdio"]);
    // TODO cmd.args(["-vga", "std"]);
    cmd.args([
        "-drive",
        &format!(
            "if=pflash,format=raw,readonly=on,file={}",
            code.to_str().unwrap()
        ),
    ]);
    cmd.args([
        "-drive",
        &format!(
            "if=pflash,format=raw,readonly=on,file={}",
            vars.to_str().unwrap()
        ),
    ]);
    cmd.args([
        "-drive",
        &format!("format=raw,file=fat:rw:{}", esp.to_str().unwrap()),
    ]);

    // Add a drive for accessing the ext4 filesystem.
    cmd.args(["-device", "virtio-scsi-pci,id=scsi"]);
    cmd.args(["-device", "scsi-hd,drive=hd"]);
    cmd.args([
        "-drive",
        &format!(
            "if=none,id=hd,format=raw,file={}",
            filesystem.to_str().unwrap()
        ),
    ]);

    run_cmd(&mut cmd)?;

    todo!();
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    println!("{cmd:?}");
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        bail!("command failed: {status}");
    }
}

fn build_benchtool() -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        "x86_64-unknown-uefi",
        "--release",
        "-p",
        "benchtool",
    ]);
    run_cmd(&mut cmd)
}

fn build_esp() -> Result<PathBuf> {
    let esp = Path::new("target/esp");
    let boot = esp.join("efi/boot");
    fs::create_dir_all(&boot)?;
    fs::copy(
        "target/x86_64-unknown-uefi/release/benchtool.efi",
        boot.join("bootx64.efi"),
    )?;

    Ok(esp.to_owned())
}
