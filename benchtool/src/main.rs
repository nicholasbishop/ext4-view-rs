#![no_main]
#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::error::Error;
use ext4_view::{Ext4, Ext4Error, Ext4Read};
use uefi::boot::{OpenProtocolAttributes, OpenProtocolParams, ScopedProtocol};
use uefi::proto::media::block::BlockIO;
use uefi::proto::media::disk::DiskIo;
use uefi::runtime::ResetType;
use uefi::{Handle, Status, boot, println, runtime};

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
            .map_err(|err| Box::new(err))?)
    }
}

fn get_media_id(handle: Handle) -> uefi::Result<u32> {
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
    println!("last lba: {}", bio.media().last_block());
    Ok(bio.media().media_id())
}

fn walk(fs: &Ext4, path: ext4_view::Path<'_>) -> Result<(), Ext4Error> {
    let entry_iter = match fs.read_dir(path) {
        Ok(entry_iter) => entry_iter,
        Err(Ext4Error::Encrypted) => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    for entry in entry_iter {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }

        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            // Read the symlink target.
            let _target = fs.read_link(&path)?;
        } else if file_type.is_dir() {
            // Recurse.
            walk(fs, path.as_path())?;
        } else {
            // Read the file.
            let _data = fs.read(&path)?;
        };
    }

    Ok(())
}

#[uefi::entry]
fn main() -> Status {
    // Find all diskio devices
    let handles = boot::find_handles::<DiskIo>().unwrap();
    println!("{}", handles.len());

    for handle in handles {
        let media_id = if let Ok(media_id) = get_media_id(handle) {
            media_id
        } else {
            continue;
        };

        if let Ok(io) = boot::open_protocol_exclusive::<DiskIo>(handle) {
            if let Ok(fs) = Ext4::load(Box::new(Disk { media_id, io })) {
                println!("starting walk...");

                walk(&fs, ext4_view::Path::new("/")).unwrap();

                println!("walk complete");

                runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
            }
        }
    }

    panic!("failed to open ext4 filesystem");
}
