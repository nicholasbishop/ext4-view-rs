# Release process

1. `git checkout main && git pull`
2. `git checkout -b <some-branch-name>`
3. Update the `version` field in `Cargo.toml`.
4. Run `cargo build` so that `Cargo.lock` gets updated.
5. Commit `Cargo.toml` and `Cargo.lock`. The commit message must start
   with `release:`.
6. Push the branch and create a PR.

When the PR is merged, the new release will automatically be created on
<https://crates.io>. A git tag will also be created automatically.

See <https://crates.io/crates/auto-release> for more details of how the
release process is implemented.
