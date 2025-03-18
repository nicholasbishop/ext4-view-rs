// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod walk;

use anyhow::Result;
use ext4_view::Ext4;
use ovmf_prebuilt::{Arch, FileType, Prebuilt, Source};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use xtask::run_cmd;

/// Run a simple wall-time performance benchmark.
pub fn run_bench(path: &Path, iters: u32) -> Result<()> {
    let vm_inputs = prep_vm()?;

    bench_impl(iters, || {
        // Load the filesystem and recursively walk all directories and
        // files. Each file is fully read and hashed.
        let ext4 = Ext4::load_from_path(path).unwrap();
        let digest = walk::walk(&ext4).unwrap();
        println!("filesystem hash: {digest}");
    });

    bench_impl(iters, || {
        // Run a VM with the filesystem attached as a disk. The VM runs
        // a UEFI application which recursively walks all directories
        // and reads each file.
        run_vm(&vm_inputs, path).unwrap()
    });
    Ok(())
}

/// Run `f` a total of `iters` times. Measure the duration of each
/// iteration, and report statistics.
fn bench_impl<F>(iters: u32, f: F)
where
    F: Fn(),
{
    let mut durations = Vec::new();
    for i in 1..=iters {
        println!("iter {i}:");

        let start = SystemTime::now();
        f();
        let duration = SystemTime::now().duration_since(start).unwrap();

        println!("{duration:.2?}");
        durations.push(duration);
    }
    durations.sort();

    let min = durations[0];
    let max = durations.last().unwrap();
    let median = durations[durations.len() / 2];
    println!("range: {min:.2?} - {max:.2?}");
    println!("median: {median:.2?}");
}

fn run_vm(inputs: &VmInputs, filesystem: &Path) -> Result<()> {
    let code = inputs.prebuilt.get_file(Arch::X64, FileType::Code);
    let vars = inputs.prebuilt.get_file(Arch::X64, FileType::Vars);

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.args(["-nodefaults", "--enable-kvm"]);
    cmd.args(["-machine", "q35"]);
    cmd.args(["-m", "1G"]);
    cmd.args(["-serial", "stdio"]);
    cmd.args(["-display", "none"]);
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
        &format!("format=raw,file=fat:rw:{}", inputs.esp.to_str().unwrap()),
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

    Ok(())
}

struct VmInputs {
    esp: PathBuf,
    prebuilt: Prebuilt,
}

fn prep_vm() -> Result<VmInputs> {
    build_uefibench()?;
    let esp = build_esp()?;

    let prebuilt = Prebuilt::fetch(Source::LATEST, "target/ovmf")
        .expect("failed to update prebuilt");

    Ok(VmInputs { esp, prebuilt })
}

fn build_uefibench() -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        "x86_64-unknown-uefi",
        "--release",
        "-p",
        "uefibench",
    ]);
    run_cmd(&mut cmd)
}

fn build_esp() -> Result<PathBuf> {
    let esp = Path::new("target/esp");
    let boot = esp.join("efi/boot");
    fs::create_dir_all(&boot)?;
    fs::copy(
        "target/x86_64-unknown-uefi/release/uefibench.efi",
        boot.join("bootx64.efi"),
    )?;

    Ok(esp.to_owned())
}
