# Changelog

## Unreleased

* Add `impl From<Corrupt> for Ext4Error`.
* Add `impl From<Incompatible> for Ext4Error`.

## 0.6.1

* Fixed a panic when loading an invalid superblock.

## 0.6.0

* MSRV increased to `1.81`.
* The error types now unconditionally implement `core::error::Error`.
* The `IoError` trait has been removed. `Ext4Read::read` now returns
  `Box<dyn Error + Send + Sync + 'static>`, and that same type is now
  stored in `Ext4Error::Io`.
