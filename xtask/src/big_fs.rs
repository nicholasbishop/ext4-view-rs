// Copyright 2024 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use crate::test_data_dir;
use anyhow::Result;
use gpt_disk_io::gpt_disk_types::BlockSize;
use gpt_disk_io::{BlockIoAdapter, Disk};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;
use tar::Archive;
use tempfile::TempDir;
use ureq::Agent;
use xtask::calc_file_sha256;

/// Download a ChromiumOS image and extract its root and stateful
/// partitions. The root partition is ext2, the stateful partition is
/// ext4. Each can be used with the `diff-walk` action to verify that
/// the library can read the whole filesystem correctly.
///
/// There's nothing particularly special about these filesystems, it's
/// just a convenient way to get a big real-world example of ext2/ext4
/// data.
pub fn download_big_filesystems() -> Result<()> {
    let root_path = test_data_dir()?.join("chromiumos_root.bin");
    let stateful_path = test_data_dir()?.join("chromiumos_stateful.bin");
    if root_path.exists() && stateful_path.exists() {
        println!(
            "{} and {} already exist, operation canceled",
            root_path.display(),
            stateful_path.display()
        );
        return Ok(());
    }

    let tmp_dir = TempDir::new_in(test_data_dir()?)?;

    let base_file_name = "chromiumos_test_image";
    let bin_file_name = format!("{base_file_name}.bin");
    let tar_file_name = format!("{base_file_name}.tar");
    let compressed_file_name = format!("{base_file_name}.tar.xz");

    // Pin to a particular version so we know we're always downloading a
    // known thing.
    let bucket = "chromiumos-image-archive";
    let board = "amd64-generic-public";
    let version = "R127-15907.0.0";
    let expected_sha256 =
        "9e1b25a4e509c9fccd62d074d963e0fda718ef0e06403e9a0a0804eb90a53b31";

    let url = format!(
        "https://storage.googleapis.com/{bucket}/{board}/{version}/{compressed_file_name}"
    );

    let download_path = tmp_dir.path().join(compressed_file_name);
    let tar_path = tmp_dir.path().join(tar_file_name);
    let bin_path = tmp_dir.path().join(&bin_file_name);

    // Download the compressed tarball.
    {
        println!("downloading {url} to {}", download_path.display());
        let agent = Agent::config_builder()
            .user_agent("https://github.com/nicholasbishop/ext4-view-rs")
            .build()
            .new_agent();
        let mut response = agent.get(&url).call()?.into_body().into_reader();
        let mut download_file = File::create(&download_path)?;
        io::copy(&mut response, &mut download_file)?;
    }

    // Validate hash.
    let actual_sha256 = calc_file_sha256(&download_path)?;
    assert_eq!(
        actual_sha256, expected_sha256,
        "sha256 of downloaded file is wrong"
    );

    // Decompress.
    {
        println!(
            "decompressing {} to {}",
            download_path.display(),
            tar_path.display()
        );
        let mut download_file = BufReader::new(File::open(download_path)?);
        let mut tar_file = File::create(&tar_path)?;
        lzma_rs::xz_decompress(&mut download_file, &mut tar_file)?;
    }

    // Untar.
    {
        println!("untarring {}", tar_path.display());
        let tar_file = File::open(tar_path)?;
        let mut archive = Archive::new(tar_file);
        let mut entry = archive
            .entries()?
            .find_map(|e| {
                let e = e.unwrap();
                if e.path_bytes() == bin_file_name.as_bytes() {
                    Some(e)
                } else {
                    None
                }
            })
            .unwrap();
        entry.unpack(&bin_path)?;
    }

    // Extract the root and stateful partitions from the disk image.
    extract_partition(&bin_path, &root_path, "ROOT-A")?;
    extract_partition(&bin_path, &stateful_path, "STATE")?;

    Ok(())
}

fn extract_partition(
    disk_path: &Path,
    output_path: &Path,
    partition_name: &str,
) -> Result<()> {
    // Extract the partition from the disk image.
    let mut bin_file = File::open(disk_path)?;
    let bs = BlockSize::BS_512;
    let mut block_buf = vec![0; bs.to_usize().unwrap()];
    // Parse the GPT to find the partition's LBA range within the disk.
    let lba_range = {
        let block_io = BlockIoAdapter::new(&mut bin_file, bs);
        let mut disk = Disk::new(block_io)?;
        let gpt = disk.read_primary_gpt_header(&mut block_buf)?;
        let layout = gpt.get_partition_entry_array_layout()?;
        let entry = disk
            .gpt_partition_entry_array_iter(layout, &mut block_buf)?
            .find(|e| {
                e.as_ref().unwrap().name == partition_name.parse().unwrap()
            })
            .unwrap()?;
        entry.lba_range().unwrap()
    };
    println!("{partition_name} partition LBA range: {lba_range}");
    // Copy the partition to the output file.
    bin_file.seek(SeekFrom::Start(
        *lba_range.to_byte_range(bs).unwrap().start(),
    ))?;
    println!(
        "writing {partition_name} partition to {}",
        output_path.display()
    );
    let mut output_file = File::create(output_path)?;
    for _ in 0..lba_range.num_blocks() {
        bin_file.read_exact(&mut block_buf)?;
        output_file.write_all(&block_buf)?;
    }

    Ok(())
}
