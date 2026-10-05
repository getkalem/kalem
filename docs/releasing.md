# Releasing Kalem

Releases are built by [cargo-dist](https://opensource.axo.dev/cargo-dist/) from the configuration in the workspace `Cargo.toml` (`[workspace.metadata.dist]`), and by the steps below for what it does not do. Publishing is the maintainers' step: nothing here runs without them.

## Once: the release workflow

```sh
cargo install cargo-dist --locked --version 0.28.0
dist generate
```

`dist generate` writes `.github/workflows/release.yml` from the configuration. Commit it. Run it again after changing the configuration or upgrading cargo-dist; `dist plan` lists the archives a tag would build. The configuration names a runner for each target (gpui is not cross-compiled, and cargo-dist's default `ubuntu-20.04` is retired) and the windowing libraries the Linux runners install.

## Each release

1. Update `version` in `[workspace.package]` of `Cargo.toml` and the workspace dependencies' `version` fields.
2. Rename `## [Unreleased]` in `CHANGELOG.md` to `## [0.1.0] - DATE` and start a new empty `## [Unreleased]` above it: cargo-dist takes the release notes from the section of the tagged version.
3. Run the checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and the manual checklist in `docs/release-checklist.md`. Run the *Released plugins* workflow from the Actions tab (or `tools/check-released-plugins.sh target/release/kalem`): every component plugin in the index must run with the release, or be released again first.
4. Tag and push: `git tag v0.1.0 && git push --tags`. The release workflow builds the archives, installers and the Homebrew formula for the targets in the configuration and makes a draft GitHub release.

## What cargo-dist does not build

- **Terminal-only archives.** Built with the `tui` feature only, without windowing or GPU libraries and without the `viewers` and `plugins` features (a third of the binary), by `.github/workflows/release-terminal.yml` when the release is published (or by hand from the Actions tab, given the tag), as `kalem-terminal-TARGET.tar.gz` (`.zip` on Windows). The CI job `terminal-only` checks that this build has no gpui in its dependency tree.

- **macOS app.** `tools/macos-app.sh --target aarch64-apple-darwin` (and `x86_64-apple-darwin`) builds `Kalem.app`. It is not signed yet: users open it with right-click and *Open* the first time. Signing, notarization and a Homebrew cask come in phase 2 (§17).

## Crates

- `org-syntax` can be published once decision D18 (entity table provenance) is made.
- gpui comes from crates.io (`gpui-unofficial`), so it no longer keeps `kalem-ui` and `gpui-rich-text` off crates.io. Git dependencies still do: `kalem-core` uses the comrak fork (`getkalem/comrak`), and `kalem-cli` the bundled plugins of `getkalem/plugins` (and through them the IronCalc fork), so `kalem-editor`, `kalem-ui` and `kalem-cli` cannot be published yet; `gpui-rich-text` stays `publish = false` until its name and API are settled. The binaries are the distribution meanwhile.
