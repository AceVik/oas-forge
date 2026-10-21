# Contributing

Thank you for helping! Bug reports, documentation fixes and features are all
welcome.

## Reporting a bug

Open an issue with the smallest input that shows the problem: the Rust
source (or doc comment), the generator options and the output you expected.
Include the diagnostic codes you see (`OF…`). Security problems go to
[SECURITY.md](SECURITY.md) instead.

## Making a change

1. Read [AGENTS.md](AGENTS.md) and [the architecture](docs/architecture.md):
   they explain the pipeline, where code belongs and the rules every change
   follows.
2. Write the test first. A new syntax or behaviour gets a corpus case in
   `tests/corpus/cases/<name>/` (a `case.toml` and `src/lib.rs`); a stage
   detail gets a unit test in the mirror file under `tests/unit/`.
3. Make the change in the stage it belongs to.
4. Review snapshot changes with `cargo insta review`. Accept only intended
   changes and describe user-visible ones in the changelog.
5. Update the guide in `docs/` when syntax or options change. Examples
   marked `<!-- corpus: name -->` must stay identical to their corpus case.
   New items get doc comments; public ones an example that runs.
6. Run the checks:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   cargo deny check
   cargo build --manifest-path examples/Cargo.toml
   typos
   ```

For larger changes, `cargo mutants --package oas-forge --test-tool nextest
-f src/<file>.rs` shows behaviour the tests do not pin down.

Tools: [`cargo-insta`](https://insta.rs/docs/cli/),
[`cargo-mutants`](https://mutants.rs),
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny),
[`typos`](https://github.com/crate-ci/typos) and, for coverage,
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov).

## Pull requests

Keep a pull request to one topic, with commits that each build and pass the
tests. Describe what changes for users. CI runs the checks above on Linux,
macOS and Windows, builds with the minimum supported Rust version (1.89) and
the minimal dependency versions, and audits the workflows. Once a week it also
tests the newest dependency releases on stable and beta Rust.

By contributing you agree that your work is licensed under the
[MIT license](LICENSE).

## Releasing

Both crates (`oas-forge` and `oas-forge-cli`) share the workspace version and
are released together by `.github/workflows/release.yml` when a version tag
is pushed.

### One-time setup

1. GitHub, *Settings → Environments*: create `crates-io` and restrict its
   deployments to tags matching `v*` (optionally add required reviewers for
   a manual approval before anything is published).
2. GitHub, *Settings → Rules*: protect `main` (pull requests, the CI checks
   must pass) and `v*` tags (only maintainers may create them).
3. crates.io, each crate's *Settings → Trusted Publishing*: add GitHub
   repository `AceVik/oas-forge`, workflow `release.yml`, environment
   `crates-io`, and enable *Require trusted publishing for all new
   versions*.
4. A crate that is not on crates.io yet cannot use trusted publishing for its
   first release. For that release only, create a crates.io API token with
   the scope `publish-new` for that crate alone (crate pattern
   `oas-forge-cli`) and store it as the secret `CRATES_IO_BOOTSTRAP_TOKEN` of
   the `crates-io` environment. The workflow uses it for nothing else: crates
   that exist are always published through trusted publishing. After the
   release, do step 3 for the new crate, delete the secret and revoke the
   token.

### Each release

1. Set the new version in `[workspace.package]` of the root `Cargo.toml`
   (for a new minor or major version, also the `oas-forge` requirement in
   `crates/oas-forge-cli/Cargo.toml`), run `cargo update -w`.
2. Rename the `Unreleased` section of `CHANGELOG.md` to
   `## [x.y.z] — YYYY-MM-DD`; it becomes the release notes.
3. Merge that to `main` through a pull request.
4. Tag the merge commit and push the tag:

   ```bash
   git tag -a v0.2.0 -m "oas-forge 0.2.0"
   git push origin v0.2.0
   ```

The workflow checks that the tag is the workspace version, that the commit
is on `main`, that CI passed for it (waiting up to an hour for a running CI)
and that the changelog has the section. It then builds the CLI for six
targets, publishes `oas-forge` and `oas-forge-cli` to crates.io and creates
the GitHub release with the binaries, `SHA256SUMS` and build provenance.
Every job can be re-run: versions already on crates.io are skipped and an
existing GitHub release gets its assets replaced. A tag can also be released
again by hand (*Actions → Release → Run workflow*).
