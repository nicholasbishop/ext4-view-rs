# Changelog

## Unreleased

* MSRV increased to `1.81`.
* The error types now unconditionally implement `core::error::Error`.
* The `IoError` trait has been removed. `Ext4Read::read` now returns
  `Box<dyn Error + Send + Sync + 'static>`, and that same type is now
  stored in `Ext4Error::Io`.