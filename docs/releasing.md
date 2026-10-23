# Releasing Kalem

Releases are built by [cargo-dist](https://opensource.axo.dev/cargo-dist/) from the configuration in the workspace `Cargo.toml` (`[workspace.metadata.dist]`), and by the steps below for what it does not do. Publishing is the maintainers' step: nothing here runs without them.

## Once: the release workflow

```sh
cargo install cargo-dist --locked --version 0.28.0
dist generate
```

`dist generate` writes `.github/workflows/release.yml` from the configuration. Commit it. Run it again after changing the configuration or upgrading cargo-dist, with the line `allow-dirty = ["ci"]` taken out for the run (it lets Dependabot update the workflow's actions, and makes `dist generate` leave the file alone); `dist plan` lists the archives a tag would build, and `dist build --artifacts=local` builds the host's. The archives are built with the `dist` profile (`[profile.dist]`, the release profile), as CI's binary size job measures them. The configuration names a runner for each target (gpui is not cross-compiled, and cargo-dist's default `ubuntu-20.04` is retired), the windowing libraries the Linux runners install, and `release-terminal.yml` as a job run after the release is announced, and `release-gate.yml` as a job the builds wait for (`plan-jobs`, with the permissions it needs in `github-custom-job-permissions`: below, *A tag only after green CI*).

## Each release

0. `main`'s CI is green on the commit to tag (the release workflow checks it again: below), and the plugin releases this version ships are published (getkalem/plugins: a tag `NAME-vX.Y.Z` each, one at a time).
1. Update `version` in `[workspace.package]` of `Cargo.toml` and the `version` field of every workspace dependency in `[workspace.dependencies]`, then `cargo check --workspace` so that `Cargo.lock` takes the new versions (the builds use `--locked`); `kalem --version` then says it. For the first release, take "once 0.1 is out" out of the README and "Kalem has no releases yet" out of `book/part-1/installing.org`.
2. Rename `## [Unreleased]` in `CHANGELOG.md` to `## [0.1.0] - DATE` and start a new empty `## [Unreleased]` above it: cargo-dist takes the release notes from the section of the tagged version, so keep it to what a user reads (GitHub takes at most 125,000 characters, and the workflow passes the notes through an environment variable); the history of each change belongs in `docs/history/`.
3. No plugin is built into a release (the feature `components` is off by default and in every release build): its users install the plugins of the index, so the plugin releases this version needs are those step 0 asks to be published, and step 4's *Released plugins* workflow checks that they run with it. `crates/kalem-components/components.toml` pins what a build of one's own with `components` builds in, and the tests.
4. Run `tools/third-party-licenses.sh` and commit `THIRD-PARTY-LICENSES.md` when it changed: every archive carries it. Then run the checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and the manual checklist in `docs/release-checklist.md`. Run the *Released plugins* workflow from the Actions tab (or `tools/check-released-plugins.sh target/release/kalem`): every component plugin in the index must run with the release, or be released again first.
5. `dist plan` lists what the tag will build. A first tag of a version can be a prerelease (`v0.1.0-rc.1`), which dist marks so.
6. Tag and push that tag alone: `git tag v0.1.0 && git push origin v0.1.0`. The release workflow waits for CI on the tagged commit (below), then builds the archives and the shell and PowerShell installers for the targets in the configuration, publishes the GitHub release with the changelog's section as its notes, and then runs `release-terminal.yml`, which attaches the terminal archives.

## A tag only after green CI

A release is built only from a commit CI (`ci.yml`) passed on: the version commit of 0.6.47 (`ed8d2fbf`) was red (run 39214933475) and its fix came in the next commit, which is the one tagged, once CI passed there. Tag the commit whose run you saw pass, and read the run's conclusion (`gh run view RUN_ID --json conclusion`), not the exit status of `gh run watch`, which is 0 for a cancelled run.

The workflows check it too. `.github/workflows/release-gate.yml` resolves the tag to its commit, lists the runs of `ci.yml` on that commit through the GitHub API, and passes when one of them succeeded, or when the only jobs of one that did not pass are macOS test jobs cancelled because no runner took them (GitHub's annotation on each: "The job was not acquired by Runner"; the owner's exception of 2026-10-14, the macOS tests then passing on a local run before the tag), which it passes with a warning naming the run; a run still queued or going is waited for (up to two and a half hours), and a run not started yet for ten minutes, so a tag pushed with its commit waits rather than fails. Otherwise it stops with an error naming the commit, each run's conclusion and link, and what to do; the release workflow's builds (`needs: custom-release-gate`) and so the GitHub release, the installers and the terminal archives are never made. `release-terminal.yml` runs the same gate first, which matters when it is run by hand for a tag.

When the gate stops a release, nothing was published:

- CI failed: fix `main`, and tag a commit CI passed on. As no release exists, the tag may move to that commit (`git tag -f v0.1.0 FIX && git push -f origin v0.1.0`) when its version is still the one in `Cargo.toml`.
- CI's run was cancelled (a newer push to `main` replaced it while it waited) or failed by chance: `gh run rerun RUN_ID`, and once it passed, re-run the release's failed jobs from the Actions tab (`gh run rerun RELEASE_RUN_ID --failed`).
- CI never ran on the commit (one not pushed to `main`): `gh workflow run ci.yml --ref v0.1.0` runs it on the tag; then re-run the release's failed jobs.

The gate is a dist plan job, so `dist generate` keeps it: it lives in `Cargo.toml` (`plan-jobs = ["./release-gate"]`, and `actions: read` and `checks: read` for it and for `release-terminal` in `[workspace.metadata.dist.github-custom-job-permissions]`), not as a hand edit of `release.yml`.

## What cargo-dist does not build

- **Terminal-only archives.** Built with the `tui` feature only, without windowing or GPU libraries and without the `components` and `plugins` features, by `.github/workflows/release-terminal.yml`, which the release workflow runs once the release is announced (dist's `post-announce-jobs`; by hand from the Actions tab, given the tag, for a release that exists), as `kalem-terminal-TARGET.tar.xz` (`.zip` on Windows) with a `.sha256` beside each, Windows's linked statically as dist links its own. CI's Emacs job (`differential test against Emacs`) builds it on a runner without those libraries and checks that gpui is not in its dependency tree.

- **macOS app.** `tools/macos-app.sh --target aarch64-apple-darwin` (and `x86_64-apple-darwin`) builds `Kalem.app`. It is not signed yet: users open it with right-click and *Open* the first time. Signing, notarization and a Homebrew cask come in phase 2 (§17); the Homebrew formula waits for a tap (`getkalem/homebrew-tap` and its token).

## Crates

- `org-syntax` and the Org crates on it can be published as far as decision D18 goes: the entity table is made from standards (`tools/gen-entities.py`); they are published after 0.1.
- gpui comes from crates.io (`gpui-unofficial`), so it no longer keeps `kalem-ui` and `gpui-rich-text` off crates.io. Git dependencies still do: `kalem-core` uses the comrak fork (`getkalem/comrak`), so `kalem-core` and what depends on it cannot be published yet (the bundled plugins are no longer a git dependency: their released components are downloaded by `kalem-components`' build); `gpui-rich-text` stays `publish = false` until its name and API are settled. The binaries are the distribution meanwhile.
