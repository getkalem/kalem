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

- [x] **C1. Only `main` saves caches.** `save-if: ${{ github.ref ==
  'refs/heads/main' }}` on every `Swatinem/rust-cache` of `ci.yml` and
  `book.yml`, the two workflows that run on pull requests (the others
  run only on `main` or by hand). Pull requests go on restoring `main`'s
  caches. Also a `workflow_dispatch` trigger on CI, so a branch can be
  measured without a pull request (such a run restores `main`'s caches
  and saves none).
  - *Measured by* `gh cache list` after dependabot's next pull requests.
  - *Done when* every Rust cache listed is `refs/heads/main`'s.
  - *Results.* Every Rust cache dependabot's pull requests made after
    the change is gone from the list; three older pull requests saved
    the Book's on 2026-10-09 still, their merge commits holding the
    `book.yml` of the day before (they stop once rebased). Pushed in
    `73370ea`.

- [x] **C2. `main`'s caches fit in 10 GB.** Two cuts:
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
  - *Results.* Line tables made the caches only 15 to 20% smaller (the
    Ubuntu test job's 1.69 to 1.37 GB, macOS's 1.22 to 1.08, Windows's
    1.39 to 1.13), but a cold compile much shorter: Ubuntu 12:28 to
    6:58, macOS 12:49 to 9:36, Windows 18:42 to 16:33 (runs 37885494037
    and 37887755617, both cold). `main`'s caches still came to 10.4 GB,
    so two more cuts (`0f44f7b`):
    - The `bundled plugins as components` job (1.24 GB) went: the
      workspace's run already held all five of its tests, on every
      runner; the one it alone built, the terminal-only Kalem with the
      components, is a step of the Ubuntu test job.
    - The daily check of the released plugins (`plugins.yml`, 1 GB)
      keeps no cache: no one waits for it.
  - Left: about 8.1 GB, every job of a push to `main` with the Book and
    the fuzzers. The caches of the removed jobs and the old keys
    (3.6 GB) are used no more and go first when GitHub evicts.

- [x] **C3. One dependabot pull request per ecosystem.** Each of
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
  - *Results.* Dependabot opened one pull request for the Cargo group
    (insta and objc2, replacing objc2's own) and one for Actions (six
    updates), both green; `fontdb`, `getrandom`, `wit-bindgen` and
    `hayro-svg` came one by one as 0.x minor updates. Three older
    Actions pull requests (upload-artifact 7, cache 6, download-artifact
    8) stay open beside the group's, which asks for lower versions of
    two of them; closing one by hand would make dependabot skip that
    version later, so they are left to the owner. `wasmparser` was
    ignored for a while on a wrong reading (that its update would fail
    the group); it is back to its own pull requests.

## 2. The test jobs

- [x] **C4. cargo-nextest runs the tests.** nextest runs every test of
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
  - *Results.* First run (37887097619): slower, 596 s for the tests on
    Ubuntu against `cargo test`'s 259 s. Every one of kalem-tui's 56
    workbook tests compiled the workbook's component in its own process
    (15 s each on a loaded runner), and that load slowed every other
    test. `kalem_components::viewer` now keeps compiled components in
    the folder `KALEM_COMPONENT_CACHE` names, which CI sets, and the
    cache's temporary file has the process's name (`62e282b`).
  - Then, the tests alone (the step less its compile):

    | Runner | `cargo test` | nextest | nextest, shared cache |
    |---|---|---|---|
    | Ubuntu | 259 s | 596 s | 238 to 298 s |
    | macOS | 425 s | 430 s | 206 to 319 s |
    | Windows | 586 s | 702 s | 212 to 425 s |

    The same 1,516 to 1,536 tests run as before, the doc tests apart.
    Ubuntu gains least: its four virtual cores are two physical ones,
    and a long test such as `latex_corpus` (32 s under `cargo test`)
    takes up to 130 s with every core busy.
  - On Windows one run failed `kalem-ui`'s `this_file_keys`, which
    waits three seconds for a file to reach the tests' trash, under
    the load of the compiles above (see C7).

- [x] **C5. The sample plugins under nextest.** kalem-script's and
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
  - *Results.* No change needed. Under nextest kalem-script's plugin
    tests take 0.2 to 1 s each, the lock and cargo's check of a built
    plugin included; the two that took 22 s on a cold runner were the
    first to build the plugins' dependencies and the one waiting for
    it. With a warm cache that build is 3 s.

- [x] **C6. Windows links with rust-lld.** `link.exe` links 106 test
  binaries; LLD is usually several times faster at it. In the Windows
  test job only: `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld`.
  - *Measured by* the Windows job's compile time with and without it.
  - *Done when* kept if it saves a minute or more and every test passes;
    otherwise taken out again and its numbers written here.
  - *Results.* Not kept. On a cold Windows runner (branch run
    37892279254 against `main`'s 37887755617) the stretch after the
    last crate, the test binaries' compile and link, took 237 s with
    LLD and 279 to 291 s with `link.exe`: about 50 s saved, under the
    minute asked. Since C7's split, Windows's halves (7 to 10 minutes)
    end with macOS's job (10 minutes), so LLD would not shorten a run
    unless macOS's tests were split too. The change is the one line
    above, set before the cache step on Windows only.

## 3. After those

- [x] **C7. Measure again and shorten the new longest job.** The test
  jobs should come out near 10 minutes. Then `binary size` (two release
  builds with LTO, 9 to 10 minutes) and `pdflatex` (9 minutes) are
  next. If one of them is the longest job by two minutes or more:
  `binary size`'s two builds as two jobs of a matrix if C2 left room in
  the caches for a second one; `pdflatex` from its step times.
  - *Done when* the numbers of a warm run on `main` are in the table
    below and no single job leads the others by more than two minutes,
    or the reason it still does is written here.
  - *Results.* With C1 to C5 the Windows test job still decided most
    runs, and it varied most: on the same cache its compile took 3:23
    to 5:26 and its tests 212 to 397 s from one run to the next. Three
    changes:
    - Windows's tests run in two halves on two runners
      (`--partition hash:1/2` and `2/2`), each building everything from
      the one cache (`bd1b238`): its tests take 2.3 to 3 minutes a half.
    - The terminal-only Kalem with the bundled plugins (C2's step of the
      Ubuntu test job) is a job of its own, restoring the Ubuntu test
      job's cache and saving none (`0bc2d08`): 5 minutes, beside the
      others.
    - nextest tries a failed test twice more (`2fa650a`): on Windows,
      `code_completions_taken` failed its first try in two runs in a row
      (11 s waiting for a completion a second try had in 1.2 s), and
      `this_file_keys` once. The log names such a test FLAKY.
    - Why `code_completions_taken` failed: its completer, standing for
      a language server, ran on a thread of its own with the default
      budget of 100 ms (a language server's is 1.5 s). A thread that
      starts later than that on a loaded runner has its items dropped
      by design, the menu stays empty, and the test's loop ran out after
      ten seconds; a completer made 200 ms late fails the same way on
      any machine. The test completers of both editors now have five
      seconds, and their waits are ten seconds by the clock rather than
      a count of turns. `this_file_keys` waits ten seconds too, and
      names the editor's status line when the file is still there: a
      move refused by Windows (a virus scan holding the file just
      copied) would show there; no cause is known yet. Since then a move
      on Windows tries again for up to a second while another process
      has the file open (kalem-fs, `retry_held`), which such a scan
      would have failed.
  - Now the longest jobs are the slower of Windows's halves and macOS's
    job, about 10 minutes each, then Ubuntu's (8) and the differential
    tests and binary size (7). macOS's tests (5 minutes on 3 cores)
    could be split as Windows's were, but a free plan runs 5 macOS jobs
    at once, which dependabot's pull requests would then fill.
  - The owner asked for the split all the same (2026-10-09): macOS's
    tests run in two halves too, as Windows's do.

## Results

Start to finish, as GitHub shows a run (a push to `main` waits for the
run before it, so `cd1a67e`'s includes a wait):

| Run | Commit | Cache | Longest job | Run |
|---|---|---|---|---|
| 37829225913, before | `fd95f66` | warm | test (windows) 21 min | 21.7 min |
| 37885494037, before | `e9e5843` | cold | test (windows) 32 min | 32.6 min |
| 37887755617, C1 to C3 | `cd1a67e` | cold | test (windows) 30 min | 40.9 min |
| 37892244068, C4 | `0f44f7b` | warm | test (ubuntu) 13.4 min | 13.4 min |
| 37896269232, C4 | `2fa650a` | warm | test (windows) 14.2 min | 14.3 min |
| 37897778896, C7 | `bd1b238` | warm | test (windows, 1/2) and test (macos) 10.4 min | 10.6 min |
