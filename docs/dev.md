# Development

A few notes on useful commands to run for local development.

## Test command

```
cargo fmt --all && cargo check --all -F std && cargo test -F std && cargo test && cargo clippy --all
```

This is not exactly the same as what CI does, but if this passes there's
a pretty good chance CI will too.

## Rustdoc

```
cargo doc -F std --no-deps --open
```

You can drop the `--open` after the first run and just refresh the
browser page.

## Code coverage

```
cargo install --locked cargo-llvm-cov
cargo +nightly llvm-cov --html -F std --branch --open
```

You can drop the `--open` after the first run and just refresh the
browser page.
