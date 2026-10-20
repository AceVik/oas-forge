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
6. Run the checks:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
   cargo deny check
   cargo build --manifest-path examples/Cargo.toml
   ```

Tools: [`cargo-insta`](https://insta.rs/docs/cli/),
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny) and, for
coverage, [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov).

## Pull requests

Keep a pull request to one topic, with commits that each build and pass the
tests. Describe what changes for users. CI runs the checks above on Linux,
macOS and Windows and the minimum supported Rust version.

By contributing you agree that your work is licensed under the
[MIT license](LICENSE).
