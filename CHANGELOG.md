# Changelog

## Unreleased

* MSRV increased to `1.85`.
* Improved error messages for various directory entry corruption errors.
* Changed the `Debug` impls for stringish types (such as `DirEntryName`
  and `Path`) to wrap the string in quotes, matching `std` behavior`.
* Changed the `Debug` impl for `DirEntry` to just show the path,
  matching `std` behavior.

## 0.9.3

* Added support for TEA hashes in directory blocks.
* Fixed 128-byte inodes incorrectly triggering a corruption error.

## 0.9.2

* Added a block cache to improve performance when running in an
  environment where the OS doesn't provide a block cache.

## 0.9.1

* Added support for the `journal_incompat_revoke` feature.

## 0.9.0

* Removed `Ext4Error::as_corrupt` and `Ext4Error::as_incompatible`.
* Renamed `Incompatible::Missing` to `Incompatible::MissingRequiredFeatures`.
* Renamed `Incompatible::Incompatible` to `Incompatible::UnsupportedFeatures`.
* Removed `Incompatible::Unknown`; these errors are now reported as
  `Incompatible::UnsupportedFeatures`.
* Removed `Incompatible::DirectoryEncrypted` and replaced it with
  `Ext4Error::Encrypted`.
* Removed `impl From<Corrupt> for Ext4Error` and
  `impl From<Incompatible>> for Ext4Error`.
* Made the `Incompatible` type opaque. It is no longer possible to
  `match` on specific types of incompatibility.
* Implemented several path conversions for non-Unix platforms that were
  previously only available on Unix. On non-Unix platforms, these
  conversions will fail on non-UTF-8 input.
  * `TryFrom<&OsStr> for ext4_view::Path`
  * `TryFrom<&std::path::PathBuf> for ext4_view::Path`
  * `TryFrom<OsString> for ext4_view::PathBuf`
  * `TryFrom<std::path::PathBuf> for ext4_view::PathBuf`
* Added support for reading filesystems that weren't cleanly unmounted.

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
