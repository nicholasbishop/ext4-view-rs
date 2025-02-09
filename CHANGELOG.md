# Changelog

## Unreleased

* Removed `Ext4Error::as_corrupt`.
* Renamed `Incompatible::Missing` to `Incompatible::MissingRequiredFeatures`.
* Renamed `Incompatible::Incompatible` to `Incompatible::UnsupportedFeatures`.
* Removed `Incompatible::Unknown`; these errors are now reported as
  `Incompatible::UnsupportedFeatures`.
* Removed `Incompatible::DirectoryEncrypted` and replaced it with
  `Ext4Error::Encrypted`.
* Removed `impl From<Corrupt> for Ext4Error`.

## 0.8.0

* Added `Path::to_str` and `PathBuf::to_str`.
* Added `Ext4::label` to get the filesystem label.
* Added `Ext4::uuid` to get the filesystem UUID.
* Made the `Corrupt` type opaque. It is no longer possible to `match` on
  specific types of corruption.

## 0.7.0

* Added `File` type and `Ext4::open`. This can be used to read parts of
  files rather than reading the whole file at once with `Ext4::read`. If
  the `std` feature is enabled, `File` impls `Read` and `Seek`.
* Added `impl From<Ext4Error> for std::io::Error`.
* Added `impl From<Corrupt> for Ext4Error`.
* Added `impl From<Incompatible> for Ext4Error`.
* Made `BytesDisplay` public.
* Made the library more robust against arithmetic overflow.

## 0.6.1

* Fixed a panic when loading an invalid superblock.

## 0.6.0

* MSRV increased to `1.81`.
* The error types now unconditionally implement `core::error::Error`.
* The `IoError` trait has been removed. `Ext4Read::read` now returns
  `Box<dyn Error + Send + Sync + 'static>`, and that same type is now
  stored in `Ext4Error::Io`.
