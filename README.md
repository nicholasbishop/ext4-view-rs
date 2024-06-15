# ext4-view-rs

[![codecov.io](https://codecov.io/gh/nicholasbishop/ext4-view-rs/coverage.svg?branch=main)](https://app.codecov.io/gh/nicholasbishop/ext4-view-rs)

This repository provides a Rust crate that allows read-only access to an
[ext4] filesystem. Write access is an explicit non-goal. The crate is
`no_std`, so it can be used in embedded contexts. However, it does
require `alloc`.

[ext4]: https://en.wikipedia.org/wiki/Ext4

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE)
or [MIT license](LICENSE-MIT) at your option.

## Contributing

See the [code of conduct] and [contributing.md].

[code of conduct]: docs/code-of-conduct.md
[contributing.md]: docs/contributing.md

## Disclaimer

This project is not an official Google project. It is not supported by
Google and Google specifically disclaims all warranties as to its quality,
merchantability, or fitness for a particular purpose.
