// Copyright 2025 Google LLC
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// https://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or https://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![no_main]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::error::Error;
use ext4_view::{Ext4, Ext4Read};
use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::media::block::BlockIO;
use uefi::proto::media::disk::DiskIo;
use uefi::runtime::ResetType;
use uefi::{Handle, Status, boot, println, runtime};

mod walk {
    use uefi::println as eprintln;

    include!("../../src/bench/walk.rs");
}

struct Disk {
    media_id: u32,
    io: ScopedProtocol<DiskIo>,
}

impl Ext4Read for Disk {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        Ok(self
            .io
            .read_disk(self.media_id, start_byte, dst)
            .map_err(Box::new)?)
    }
}

fn get_media_id(handle: Handle) -> uefi::Result<u32> {
    // Safety: nothing else should be accessing the disk.
    let bio = unsafe {
        boot::open_protocol::<BlockIO>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }?;
    Ok(bio.media().media_id())
}

#[uefi::entry]
fn main() -> Status {
    let handles = boot::find_handles::<DiskIo>().unwrap();

    for handle in handles {
        let media_id = if let Ok(media_id) = get_media_id(handle) {
            media_id
        } else {
            continue;
        };

        if let Ok(io) = boot::open_protocol_exclusive::<DiskIo>(handle)
            && let Ok(fs) = Ext4::load(Box::new(Disk { media_id, io }))
        {
            println!("starting walk...");
            let digest = walk::walk(&fs).unwrap();
            println!("filesystem hash: {digest}");

            runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
        }
    }

    panic!("failed to open ext4 filesystem");
}
