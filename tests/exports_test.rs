use oas_forge::Generator;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_full_export_success() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    // Create a file with a root snippet
    fs::write(
        src_dir.join("main.rs"),
        r#"
        /// @openapi
        /// openapi: 3.0.0
        /// info:
        ///   title: Test API
        ///   version: 1.0.0
        fn main() {}
        "#,
    )
    .unwrap();

    let output_path = dir.path().join("openapi.yaml");

    Generator::new()
        .input(src_dir)
        .output(&output_path)
        .generate()
        .expect("Should generate full spec");

    let content = fs::read_to_string(output_path).unwrap();
    assert!(content.contains("title: Test API"));
}

#[test]
fn test_full_export_fail_no_root() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    // No root snippet, just a path
    fs::write(
        src_dir.join("main.rs"),
        r#"
        /// @route GET /health
        fn health() {}
        "#,
    )
    .unwrap();

    let output_path = dir.path().join("openapi.yaml");

    let result = Generator::new()
        .input(src_dir)
        .output(&output_path)
        .generate();

    assert!(result.is_err(), "Should fail without root");
}

#[test]
fn test_schema_export_relaxed() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    fs::write(
        src_dir.join("models.rs"),
        r#"
        /// @openapi
        struct User { name: String }
        "#,
    )
    .unwrap();

    let output_path = dir.path().join("schemas.yaml");

    Generator::new()
        .input(src_dir)
        .output_schemas(&output_path)
        .generate()
        .expect("Should generate schemas without root");

    let content = fs::read_to_string(output_path).unwrap();
    assert!(content.contains("User"));
    assert!(content.contains("properties"));
    assert!(!content.contains("openapi: 3.0.0")); // Should just be the map of schemas
}

#[test]
fn test_paths_export_relaxed() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    fs::write(
        src_dir.join("routes.rs"),
        r#"
        /// @route GET /ping
        fn ping() {}
        "#,
    )
    .unwrap();

    let output_path = dir.path().join("paths.yaml");

    Generator::new()
        .input(src_dir)
        .output_paths(&output_path)
        .generate()
        .expect("Should generate paths without root");

    let content = fs::read_to_string(output_path).unwrap();
    assert!(content.contains("/ping"));
    assert!(content.contains("get"));
    assert!(!content.contains("openapi: 3.0.0"));
}

#[test]
fn test_fragment_export_headless() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    // Source contains root, but we export fragment which should strip it
    fs::write(
        src_dir.join("main.rs"),
        r#"
        /// @openapi
        /// openapi: 3.0.0
        /// info: { title: "Strip Me", version: "1" }
        /// servers: [{ url: "http://localhost" }]
        fn main() {}

        /// @route GET /test
        fn test() {}
        "#,
    )
    .unwrap();

    let output_path = dir.path().join("fragment.yaml");

    Generator::new()
        .input(src_dir)
        .output_fragments(&output_path)
        .generate()
        .expect("Should generate fragment");

    let content = fs::read_to_string(output_path).unwrap();
    assert!(content.contains("/test"), "Should contain paths");
    assert!(!content.contains("Strip Me"), "Should strip info");
    assert!(!content.contains("servers"), "Should strip servers");
}

#[test]
fn test_hybrid_strategies() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir(&src_dir).unwrap();

    fs::write(
        src_dir.join("main.rs"),
        r#"
        /// @openapi
        /// openapi: 3.0.0
        /// info: { title: "Hybrid", version: "1" }
        fn main() {}

        /// @route GET /h
        fn h() {}

        /// @openapi
        struct H { f: String }
        "#,
    )
    .unwrap();

    let out_full = dir.path().join("full.yaml");
    let out_schemas = dir.path().join("schemas.yaml");
    let out_paths = dir.path().join("paths.yaml");
    let out_frag = dir.path().join("frag.yaml");

    Generator::new()
        .input(src_dir)
        .output(&out_full)
        .output_schemas(&out_schemas)
        .output_paths(&out_paths)
        .output_fragments(&out_frag)
        .generate()
        .expect("Hybrid generation failed");

    assert!(fs::exists(&out_full).unwrap());
    assert!(fs::exists(&out_schemas).unwrap());
    assert!(fs::exists(&out_paths).unwrap());
    assert!(fs::exists(&out_frag).unwrap());

    // Verify content logic
    let full = fs::read_to_string(out_full).unwrap();
    assert!(full.contains("Hybrid"));

    let schemas = fs::read_to_string(out_schemas).unwrap();
    assert!(schemas.contains("H"), "Schemas should contain struct H");
    assert!(
        !schemas.contains("/h"),
        "Schemas should NOT contain path /h"
    );

    let frag = fs::read_to_string(out_frag).unwrap();
    assert!(!frag.contains("Hybrid"), "Fragment should strip info");
}
