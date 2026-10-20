//! Documentation examples are tested: a fenced `rust` block right after a
//! `<!-- corpus: <case> -->` marker must be the `src/lib.rs` of that corpus
//! case, which builds it, validates every output against the official
//! schemas and snapshots the result.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The Markdown files of the repository that may contain examples.
fn documents() -> Vec<PathBuf> {
    let mut documents = vec![
        root().join("README.md"),
        root().join("crates/oas-forge-cli/README.md"),
    ];
    let guide = std::fs::read_dir(root().join("docs")).expect("docs/ exists");
    documents.extend(
        guide
            .map(|entry| entry.expect("readable docs/").path())
            .filter(|path| path.extension().is_some_and(|e| e == "md")),
    );
    documents.sort();
    documents
}

/// `(document, case, code)` of every marked example.
fn marked_examples() -> Vec<(PathBuf, String, String)> {
    let mut examples = Vec::new();
    for document in documents() {
        let text = std::fs::read_to_string(&document).expect("readable document");
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            let Some(case) = line
                .trim()
                .strip_prefix("<!-- corpus: ")
                .and_then(|rest| rest.strip_suffix(" -->"))
            else {
                continue;
            };
            let fence = lines.next().unwrap_or_default();
            assert!(
                fence.starts_with("```rust"),
                "{}: the marker of `{case}` must be followed by a ```rust block",
                document.display()
            );
            let code: Vec<&str> = lines.by_ref().take_while(|l| *l != "```").collect();
            examples.push((document.clone(), case.to_string(), code.join("\n") + "\n"));
        }
    }
    examples
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[test]
fn marked_examples_are_corpus_sources() {
    let examples = marked_examples();
    assert!(examples.len() >= 3, "README examples are marked");
    for (document, case, code) in &examples {
        let source = root()
            .join("tests/corpus/cases")
            .join(case)
            .join("src/lib.rs");
        let expected = std::fs::read_to_string(&source)
            .unwrap_or_else(|e| panic!("{}: no corpus case `{case}`: {e}", relative(document)));
        assert_eq!(
            code,
            &expected,
            "{}: the example `{case}` differs from {}",
            relative(document),
            relative(&source)
        );
    }
}

#[test]
fn documentation_cases_belong_to_an_example() {
    let marked: BTreeSet<String> = marked_examples().into_iter().map(|(_, c, _)| c).collect();
    let cases = std::fs::read_dir(root().join("tests/corpus/cases")).expect("corpus cases");
    for entry in cases {
        let name = entry
            .expect("readable case")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name.starts_with("readme_") || name.starts_with("guide_") {
            assert!(
                marked.contains(&name),
                "corpus case `{name}` is not the source of any documentation example"
            );
        }
    }
}
