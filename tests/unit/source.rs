//! `src/source.rs`: input discovery.

use crate::common::{Project, codes};
use oas_forge::diagnostics::{Code, Diagnostics};
use oas_forge::error::Error;
use oas_forge::source::{
    DEFAULT_MAX_FILE_SIZE, DiscoverOptions, SourceFile, SourceKind, SourceSet,
};
use std::path::{Path, PathBuf};

fn discover(
    inputs: &[PathBuf],
    includes: &[PathBuf],
    options: &DiscoverOptions,
) -> (oas_forge::Result<SourceSet>, Diagnostics) {
    let mut diagnostics = Diagnostics::new();
    let result = SourceSet::discover(inputs, includes, options, &mut diagnostics);
    (result, diagnostics)
}

fn names(set: &SourceSet, root: &Path) -> Vec<String> {
    set.files()
        .iter()
        .map(|f| {
            f.path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn kinds_are_decided_by_extension() {
    for (path, kind) in [
        ("a.rs", Some(SourceKind::Rust)),
        ("a.yaml", Some(SourceKind::Yaml)),
        ("a.yml", Some(SourceKind::Yaml)),
        ("a.json", Some(SourceKind::Json)),
        ("a.md", None),
        ("Makefile", None),
        ("a.RS", None),
    ] {
        assert_eq!(SourceKind::of(Path::new(path)), kind, "{path}");
    }
}

#[test]
fn in_memory_files_expose_their_parts() {
    let file = SourceFile::new("src/a.rs", SourceKind::Rust, "fn a() {}");
    assert_eq!(file.path(), Path::new("src/a.rs"));
    assert_eq!(file.kind(), SourceKind::Rust);
    assert_eq!(file.text(), "fn a() {}");
    assert_eq!(file.location(4).to_string(), "src/a.rs:4");

    let set = SourceSet::from_files(vec![file.clone()]);
    assert_eq!(set.files(), [file]);
    assert_eq!(set.watched(), [PathBuf::from("src/a.rs")]);
}

#[test]
fn options_have_safe_defaults_and_builders() {
    let options = DiscoverOptions::default();
    assert!(options.exclude.is_empty());
    assert_eq!(options.max_file_size, DEFAULT_MAX_FILE_SIZE);
    let options = options
        .with_exclude(vec!["x".into()])
        .with_max_file_size(10);
    assert_eq!(options.exclude, [PathBuf::from("x")]);
    assert_eq!(options.max_file_size, 10);
}

#[test]
fn directories_are_walked_recursively_in_sorted_order() {
    let project = Project::new();
    project
        .file("src/b.rs", "")
        .file("src/a/z.rs", "")
        .file("src/a.rs", "")
        .file("src/c.yaml", "")
        .file("src/notes.md", "")
        .file("other/x.yml", "");
    let (set, diagnostics) = discover(
        &[project.path("src"), project.path("other")],
        &[],
        &DiscoverOptions::default(),
    );
    let set = set.unwrap();
    // Sorted by file name per directory: `a` sorts before `a.rs`.
    assert_eq!(
        names(&set, project.root()),
        [
            "src/a/z.rs",
            "src/a.rs",
            "src/b.rs",
            "src/c.yaml",
            "other/x.yml"
        ]
    );
    assert_eq!(set.watched(), [project.path("src"), project.path("other")]);
    assert!(diagnostics.is_empty());
}

#[test]
fn includes_follow_inputs_and_missing_ones_are_reported() {
    let project = Project::new();
    project.file("src/a.rs", "").file("base.json", "{}");
    let (set, diagnostics) = discover(
        &[project.path("src")],
        &[project.path("base.json"), project.path("missing.yaml")],
        &DiscoverOptions::default(),
    );
    let set = set.unwrap();
    assert_eq!(names(&set, project.root()), ["src/a.rs", "base.json"]);
    assert_eq!(set.files()[1].kind(), SourceKind::Json);
    assert!(set.watched().contains(&project.path("base.json")));
    assert_eq!(codes(&diagnostics), [Code::MissingInclude]);
}

#[test]
fn outputs_are_never_read_back() {
    let project = Project::new();
    project
        .file("src/a.rs", "")
        .file("src/openapi.yaml", "openapi: 3.1.0");
    let options =
        DiscoverOptions::default().with_exclude(vec![project.path("src/../src/openapi.yaml")]);
    let (set, diagnostics) = discover(&[project.path("src")], &[], &options);
    assert_eq!(names(&set.unwrap(), project.root()), ["src/a.rs"]);
    assert_eq!(codes(&diagnostics), [Code::OutputNotRead]);
}

#[test]
fn relative_and_not_yet_existing_exclusions_are_compared_normalized() {
    let project = Project::new();
    project.file("src/a.rs", "");
    let options = DiscoverOptions::default().with_exclude(vec![
        project.path("missing-dir/out.yaml"),
        project.path("src/new.yaml"),
        PathBuf::from("relative.yaml"),
    ]);
    let (set, diagnostics) = discover(&[project.path("src")], &[], &options);
    assert_eq!(names(&set.unwrap(), project.root()), ["src/a.rs"]);
    assert!(diagnostics.is_empty());
}

#[test]
fn oversized_files_are_skipped_with_an_error() {
    let project = Project::new();
    project
        .file("src/big.rs", "// 0123456789")
        .file("src/small.rs", "");
    let options = DiscoverOptions::default().with_max_file_size(5);
    let (set, diagnostics) = discover(&[project.path("src")], &[], &options);
    assert_eq!(names(&set.unwrap(), project.root()), ["src/small.rs"]);
    // The limit itself is allowed.
    project.file("src/small.rs", "12345");
    assert_eq!(codes(&diagnostics), [Code::FileTooLarge]);
    assert!(diagnostics.has_errors());
    let (set, _) = discover(&[project.path("src")], &[], &options);
    assert_eq!(names(&set.unwrap(), project.root()), ["src/small.rs"]);
    assert_eq!(
        DEFAULT_MAX_FILE_SIZE,
        16 * 1024 * 1024,
        "16 MiB, as documented"
    );
}

#[test]
fn no_files_at_all_is_an_error() {
    let project = Project::new();
    project.dir("empty");
    let (result, _) = discover(&[project.path("empty")], &[], &DiscoverOptions::default());
    assert!(matches!(result, Err(Error::NoFilesFound)));

    let (result, diagnostics) = discover(
        &[],
        &[project.path("missing.yaml")],
        &DiscoverOptions::default(),
    );
    assert!(matches!(result, Err(Error::NoFilesFound)));
    assert_eq!(codes(&diagnostics), [Code::MissingInclude]);
}

#[test]
fn unsupported_files_count_as_present_but_are_not_read() {
    let project = Project::new();
    project.file("docs/readme.md", "# hi");
    let (set, _) = discover(&[project.path("docs")], &[], &DiscoverOptions::default());
    assert!(set.unwrap().files().is_empty());
}

#[test]
fn a_missing_input_directory_is_an_io_error_naming_it() {
    let project = Project::new();
    let (result, _) = discover(&[project.path("nope")], &[], &DiscoverOptions::default());
    match result {
        Err(Error::Io { path, .. }) => assert_eq!(path, project.path("nope")),
        other => panic!("expected an IO error, got {other:?}"),
    }
}

#[test]
fn unreadable_files_are_io_errors() {
    let project = Project::new();
    project.file("src/a.rs", "");
    std::fs::write(project.path("src/bad.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let (result, _) = discover(&[project.path("src")], &[], &DiscoverOptions::default());
    match result {
        Err(Error::Io { path, .. }) => assert_eq!(path, project.path("src/bad.rs")),
        other => panic!("expected an IO error, got {other:?}"),
    }
}

#[test]
fn files_are_read_once_even_when_inputs_overlap() {
    let project = Project::new();
    project
        .file("src/a.rs", "")
        .file("src/api/b.rs", "")
        .file("src/root.yaml", "");
    let (set, diagnostics) = discover(
        &[project.path("src"), project.path("src/api")],
        &[
            project.path("src/root.yaml"),
            project.path("src/../src/a.rs"),
        ],
        &DiscoverOptions::default(),
    );
    assert_eq!(
        names(&set.unwrap(), project.root()),
        ["src/a.rs", "src/api/b.rs", "src/root.yaml"]
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[cfg(unix)]
mod symbolic_links {
    use super::*;
    use std::os::unix::fs::symlink;

    /// Only links below an input are skipped with a finding; an input that
    /// cannot be read at all is an error.
    #[test]
    fn a_broken_input_is_an_error() {
        let project = Project::new();
        symlink(project.path("missing"), project.path("src")).unwrap();
        let (result, diagnostics) =
            discover(&[project.path("src")], &[], &DiscoverOptions::default());
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
        assert!(!codes(&diagnostics).contains(&Code::UnreadableLink));
    }

    #[test]
    fn linked_files_and_directories_are_read() {
        let project = Project::new();
        project
            .file("src/a.rs", "")
            .file("shared/linked.rs", "")
            .file("shared/dir/inner.rs", "");
        symlink(
            project.path("shared/linked.rs"),
            project.path("src/linked.rs"),
        )
        .unwrap();
        symlink(project.path("shared/dir"), project.path("src/dir")).unwrap();
        // A second link to a file that is already read adds nothing.
        symlink(project.path("src/a.rs"), project.path("src/z.rs")).unwrap();
        let (set, diagnostics) = discover(&[project.path("src")], &[], &DiscoverOptions::default());
        assert_eq!(
            names(&set.unwrap(), project.root()),
            ["src/a.rs", "src/dir/inner.rs", "src/linked.rs"]
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn loops_and_broken_links_are_reported_and_skipped() {
        let project = Project::new();
        project.file("src/a.rs", "");
        symlink(project.path("src"), project.path("src/loop")).unwrap();
        symlink(
            project.path("src/missing.rs"),
            project.path("src/broken.rs"),
        )
        .unwrap();
        let (set, diagnostics) = discover(&[project.path("src")], &[], &DiscoverOptions::default());
        assert_eq!(names(&set.unwrap(), project.root()), ["src/a.rs"]);
        assert_eq!(
            codes(&diagnostics),
            [Code::UnreadableLink, Code::UnreadableLink]
        );
        let locations: Vec<String> = diagnostics
            .iter()
            .map(|d| d.location().unwrap().file().display().to_string())
            .collect();
        assert!(locations[0].ends_with("broken.rs"), "{locations:?}");
        assert!(locations[1].ends_with("loop"), "{locations:?}");
    }
}

#[test]
fn walk_errors_are_reported_once() {
    let project = Project::new();
    let (result, _) = discover(&[project.path("nope")], &[], &DiscoverOptions::default());
    let message = result.unwrap_err().to_string();
    assert_eq!(message.matches("IO error").count(), 1, "{message}");
}
