# CI todo: a shorter wait for CI

Written 2026-10-09 at commit `e9e5843`, from the last ten runs of the CI
workflow (`.github/workflows/ci.yml`), the step timings of their jobs,
the logs of the three test jobs of run 37829225913 (`Version 0.5.1`), and
the repository's Actions caches (`gh cache list`).

Conventions: `[ ]` open, `[x]` done, `[~]` partly done. Work the items in
order: each says how it is measured and when it is done, and its
results are written under it once it has run on GitHub.

## Where the time goes

The 13 jobs start together, so a run lasts as long as its longest job.
A push to `main` also waits for the run before it to finish (the
workflow's concurrency group), and dependabot's pull requests share the
runners: a public repository on the free plan gets 20 jobs at once.

The longest job is always a test job, and nearly all of it is the one
`cargo test --workspace --no-fail-fast` step. With a warm cache
(run 37829225913):

| Test job | Compile | Tests | Step |
|---|---|---|---|
| Ubuntu | 5:20 | 4:19 | 9:39 |
| macOS | 3:43 | 7:25 | 11:08 |
| Windows | 10:06 | 10:05 | 20:11 |

- **Compiling** is mostly linking: 106 test binaries, each with the
  whole dependency tree (gpui, Wasmtime) and full debug info. On Ubuntu
  the libraries are done 45 s in, and the test binaries take the other
  four and a half minutes.
- **Testing** is serial between binaries: `cargo test` runs the 106 test
  binaries one after another, and only the tests inside one binary run
  in parallel. The slowest binaries hold one or two long tests each
  (Windows: `latex_corpus` 98 s, `parse` 53 s, `parity` 53 s,
  `component_speed` 38 s, `incremental` 36 s, `emacs_diff` 36 s,
  `leader_keys` 29 s), and the runner's other cores wait.
- **The sample plugins** (`tests/plugins`) are built for
  `wasm32-unknown-unknown` twice in every test job, by kalem-script's
  tests and by kalem-cli's, each into its own target folder. With their
  dependencies in the cache that is 3 and 5 seconds; a first reading of
  the log took the tests that ran between the two builds for the builds
  (about 80 s).

**The caches** are the other half. GitHub keeps 10 GB of Actions caches
per repository and evicts the least recently used. On 2026-10-08 the
repository held 10.4 GB, and on 2026-10-09 9.8 GB, of which `main` had
no test cache left: the pull requests of dependabot had saved their own
copies (1.7 GB for Ubuntu's test job alone) and pushed out `main`'s.
Every pull request can read `main`'s caches, so its own copies buy
nothing. Without its cache a job compiles everything:

| Job | Warm | Cold |
|---|---|---|
| test (macOS), run 37821338761 | 11 min | 26 min |
| bundled plugins as components, run 37829225913 | 3 min | 14 min |

Even alone, `main`'s caches would not fit: one per job, they come to
about 12 GB (the three test jobs 4.3 GB, components 1.5, binary size
1.2, clippy, terminal-only and MSRV 0.75 each, Emacs, the Book, pdflatex
and the plugins check about 0.6 each, the fuzzers 0.15).

The 91-minute run (37798353578) was a second attempt of a run; its
macOS jobs show as cancelled for that reason, not for CI's speed.

## 1. Caches

- [ ] **C1. Only `main` saves caches.** `save-if: ${{ github.ref ==
  'refs/heads/main' }}` on every `Swatinem/rust-cache` of `ci.yml` and
  `book.yml`, the two workflows that run on pull requests (the others
  run only on `main` or by hand). Pull requests go on restoring `main`'s
  caches. Also a `workflow_dispatch` trigger on CI, so a branch can be
  measured without a pull request (such a run restores `main`'s caches
  and saves none).
  - *Measured by* `gh cache list` after dependabot's next pull requests.
  - *Done when* every Rust cache listed is `refs/heads/main`'s.

- [ ] **C2. `main`'s caches fit in 10 GB.** Two cuts:
  - The `terminal-only build` job goes. The Emacs job already builds the
    terminal-only binary on a runner without the windowing libraries
    (as does the Book's), so it proves the same; the `cargo tree` check
    that gpui is not in that binary's dependency tree moves into the
    Emacs job. One job and one 750 MB cache less.
  - Line tables instead of full debug info in CI's debug builds
    (`CARGO_PROFILE_DEV_DEBUG=line-tables-only` in `ci.yml`'s
    environment). The dependencies' libraries in the caches shrink, and
    each of the 106 test binaries links faster (on Windows the PDBs are
    a large part of the link). A panic still names its file and line,
    and a backtrace keeps its line numbers.
  - *Measured by* `gh cache list` after a full run on `main` (the first
    run is cold: the environment is part of the cache's key).
  - *Done when* `main`'s caches total under 9 GB, which leaves room for
    the monthly arXiv job.

- [ ] **C3. One dependabot pull request per ecosystem.** Each of
  dependabot's pull requests runs all 13 jobs, and dependabot rebases
  every open one when `Cargo.lock` moves on `main`: eight open pull
  requests (as on 2026-10-09) are a hundred jobs ahead of `main`'s own in
  the runners' queue. Groups: Cargo's minor and patch updates in one
  pull request (gpui's group stays as it is), all of GitHub Actions'
  updates in one. Dependabot counts a 0.x crate's minor update (0.23 to
  0.24) as major, as Cargo does, so the updates that can break still
  come one by one.
  - Dependabot's Cargo pull requests are labeled `book-unchanged` (the
    label is new): the Book workflow's check that a change to the code a
    chapter describes changes the chapter failed every version bump.
  - `dtolnay/rust-toolchain` is left alone: its tags are Rust's
    versions, and dependabot offered to move the MSRV job's `@1.96` to
    `@1.120`.
  - *Done when* dependabot's next run opens one pull request per
    ecosystem besides gpui's and the major updates, and closes the
    single ones it replaces.

## 2. The test jobs

- [ ] **C4. cargo-nextest runs the tests.** nextest runs every test of
  every binary in one pool, as many at once as the runner has cores, so
  a binary with one long test no longer holds up the rest. In the test
  matrix: `cargo nextest run --workspace --profile ci`, then
  `cargo test --workspace --doc` (nextest does not run doc tests);
  nextest comes from `taiki-e/install-action` with `wasm-tools`.
  - Four test binaries have their own `main` (kalem-core's
    `lsp_service` and `plugin_install`, kalem-lsp's `fake_server`,
    kalem-ui's `latency`) and run their tests whatever they are asked;
    nextest first asks every binary `--list`. They learn to answer it,
    as one test named `main` (kalem-ui's `latency` as an ignored one),
    and keep working under `cargo test`.
  - `.config/nextest.toml`: a `ci` profile that does not stop at the
    first failure and repeats the failures' output at the end;
    kalem-cli's `component_speed`, whose ceilings are measurements,
    runs with no other test beside it.
  - *Measured by* the test step on the three runners against the table
    above, and the number of tests run against `cargo test`'s.
  - *Done when* the step is shorter on all three, with the same tests
    run, and green.

- [ ] **C5. The sample plugins under nextest.** kalem-script's and
  kalem-cli's tests build the plugins of `tests/plugins` behind a lock,
  once per test process: 3 and 5 seconds a job under `cargo test`. Under
  nextest every test is a process of its own, so each of kalem-script's
  tests that loads a plugin asks cargo again whether it is built (and
  wraps it again), one process at a time behind the lock.
  - *Measured by* the times of kalem-script's `extension`, `viewer` and
    `grid` tests and kalem-cli's `extensions` under nextest.
  - *Done when* they add no more than about ten seconds to the step, or
    a test process skips a build another process of the same run made
    (`NEXTEST_RUN_ID` names the run).

- [ ] **C6. Windows links with rust-lld.** `link.exe` links 106 test
  binaries; LLD is usually several times faster at it. In the Windows
  test job only: `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld`.
  - *Measured by* the Windows job's compile time with and without it.
  - *Done when* kept if it saves a minute or more and every test passes;
    otherwise taken out again and its numbers written here.

## 3. After those

- [ ] **C7. Measure again and shorten the new longest job.** The test
  jobs should come out near 10 minutes. Then `binary size` (two release
  builds with LTO, 9 to 10 minutes) and `pdflatex` (9 minutes) are
  next. If one of them is the longest job by two minutes or more:
  `binary size`'s two builds as two jobs of a matrix if C2 left room in
  the caches for a second one; `pdflatex` from its step times.
  - *Done when* the numbers of a warm run on `main` are in the table
    below and no single job leads the others by more than two minutes,
    or the reason it still does is written here.

## Results

| Run | Commit | Longest job | Run time |
|---|---|---|---|
| 37829225913 (before) | `fd95f66` | test (windows) 21 min | 21 min |
