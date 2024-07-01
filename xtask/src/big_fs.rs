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
use tar::Archive;
use tempfile::TempDir;
use xtask::calc_file_sha256;

/// Download a ChromiumOS image and extract its stateful partition,
/// which is an Ext4 filesystem. This can be used with the `diff-walk`
/// action to verify that the library can read the whole filesystem
/// correctly.
///
/// There's nothing particularly special about this filesystem, it's
/// just a convenient way to get a big real-world example of ext4 data.
pub fn download_big_filesystem() -> Result<()> {
    let big_fs_path = test_data_dir()?.join("chromiumos_stateful.bin");
    if big_fs_path.exists() {
        println!(
            "{} already exists, operation canceled",
            big_fs_path.display()
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

    let url = format!("https://storage.googleapis.com/{bucket}/{board}/{version}/{compressed_file_name}");

    let download_path = tmp_dir.path().join(compressed_file_name);
    let tar_path = tmp_dir.path().join(tar_file_name);
    let bin_path = tmp_dir.path().join(&bin_file_name);

    // Download the compressed tarball.
    {
        println!("downloading {url} to {}", download_path.display());
        let agent = ureq::AgentBuilder::new()
            .user_agent("https://github.com/nicholasbishop/ext4-view-rs")
            .build();
        let mut response = agent.get(&url).call()?.into_reader();
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

    // Extract the stateful partition from the disk image.
    let mut bin_file = File::open(bin_path)?;
    let bs = BlockSize::BS_512;
    let mut block_buf = vec![0; bs.to_usize().unwrap()];
    // Parse the GPT to find the stateful partition's LBA range within
    // the disk.
    let lba_range = {
        let block_io = BlockIoAdapter::new(&mut bin_file, bs);
        let mut disk = Disk::new(block_io)?;
        let gpt = disk.read_primary_gpt_header(&mut block_buf)?;
        let layout = gpt.get_partition_entry_array_layout()?;
        let stateful_entry = disk
            .gpt_partition_entry_array_iter(layout, &mut block_buf)?
            .find(|e| e.as_ref().unwrap().name == "STATE".parse().unwrap())
            .unwrap()?;
        stateful_entry.lba_range().unwrap()
    };
    println!("stateful partition LBA range: {lba_range}");
    // Copy the stateful partition to the output file.
    bin_file.seek(SeekFrom::Start(
        *lba_range.to_byte_range(bs).unwrap().start(),
    ))?;
    println!("writing stateful partition to {}", big_fs_path.display());
    let mut output_file = File::create(big_fs_path)?;
    for _ in 0..lba_range.num_blocks() {
        bin_file.read_exact(&mut block_buf)?;
        output_file.write_all(&block_buf)?;
    }
    Ok(())
}
