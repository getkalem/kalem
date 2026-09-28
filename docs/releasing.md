# Releasing Kalem

Releases are built by [cargo-dist](https://opensource.axo.dev/cargo-dist/) from the configuration in the workspace `Cargo.toml` (`[workspace.metadata.dist]`), and by the steps below for what it does not do. Publishing is the maintainers' step: nothing here runs without them.

## Once: the release workflow

```sh
cargo install cargo-dist --locked --version 0.28.0
dist generate
```

`dist generate` writes `.github/workflows/release.yml` from the configuration. Commit it. Run it again after changing the configuration or upgrading cargo-dist.

## Each release

1. Update `version` in `[workspace.package]` of `Cargo.toml` and the workspace dependencies' `version` fields.
2. Move the `[Unreleased]` entries of `CHANGELOG.md` under the new version with the date.
3. Run the checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and the manual checklist in `docs/release-checklist.md`.
4. Tag and push: `git tag v0.1.0 && git push --tags`. The release workflow builds the archives, installers and the Homebrew formula for the targets in the configuration and makes a draft GitHub release.

## What cargo-dist does not build

- **Terminal-only archives.** Built with the `tui` feature only, without windowing or GPU libraries:

  ```sh
  cargo build --release -p kalem-editor --no-default-features --features tui
  ```

  Name them `kalem-terminal-TARGET.tar.gz` and attach them to the release. The CI job `terminal-only` checks that this build has no gpui in its dependency tree.

- **macOS app.** `tools/macos-app.sh --target aarch64-apple-darwin` (and `x86_64-apple-darwin`) builds `Kalem.app`. It is not signed yet: users open it with right-click and *Open* the first time. Signing, notarization and a Homebrew cask come in phase 2 (§17).

## Crates

- `org-syntax` can be published once decision D18 (entity table provenance) is made.
- `kalem-editor`, `kalem-ui` and `gpui-rich-text` cannot go to crates.io while gpui is a git dependency (Zed's main branch, for AccessKit); `gpui-rich-text` is marked `publish = false` for that reason. The binaries are the distribution meanwhile.
