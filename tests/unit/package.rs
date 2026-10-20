//! `src/package.rs`: package metadata and the generated root document.

use crate::common::Project;
use oas_forge::Package;
use oas_forge::error::Error;
use oas_forge::package::ROOT_VERSION;
use serde_json::json;

mod manifests {
    use super::*;

    #[test]
    fn reads_the_package_section() {
        let project = Project::new();
        project.file(
            "Cargo.toml",
            "[package]\nname = \"shop\"\nversion = \"2.1.0\"\ndescription = \"A shop\"\nlicense = \"MIT OR Apache-2.0\"\nrepository = \"https://example.com/shop\"\nhomepage = \"https://shop.example.com\"\n",
        );
        let package = Package::from_manifest(project.path("Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package.name, "shop");
        assert_eq!(package.version, "2.1.0");
        assert_eq!(package.description.as_deref(), Some("A shop"));
        assert_eq!(package.license.as_deref(), Some("MIT OR Apache-2.0"));
        assert_eq!(
            package.repository.as_deref(),
            Some("https://example.com/shop")
        );
        assert_eq!(
            package.homepage.as_deref(),
            Some("https://shop.example.com")
        );
    }

    #[test]
    fn missing_fields_are_none_and_the_version_defaults() {
        let project = Project::new();
        project.file("Cargo.toml", "[package]\nname = \"bare\"\n");
        let package = Package::from_manifest(project.path("Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package, Package::new("bare", "0.0.0"));
    }

    #[test]
    fn inherited_fields_come_from_the_workspace() {
        let project = Project::new();
        project
            .file(
                "Cargo.toml",
                "[workspace]\nmembers = [\"crates/api\"]\n\n[workspace.package]\nversion = \"3.0.0\"\nlicense = \"MIT\"\nrepository = \"https://example.com/ws\"\n",
            )
            .file(
                "crates/api/Cargo.toml",
                "[package]\nname = \"api\"\nversion.workspace = true\nlicense = { workspace = true }\nrepository.workspace = true\ndescription.workspace = true\n",
            );
        let package = Package::from_manifest(project.path("crates/api/Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package.name, "api");
        assert_eq!(package.version, "3.0.0");
        assert_eq!(package.license.as_deref(), Some("MIT"));
        assert_eq!(
            package.repository.as_deref(),
            Some("https://example.com/ws")
        );
        assert_eq!(package.description, None, "not set in the workspace");
    }

    #[test]
    fn the_nearest_workspace_wins_and_non_workspace_manifests_are_skipped() {
        let project = Project::new();
        project
            .file(
                "Cargo.toml",
                "[workspace]\n[workspace.package]\nversion = \"9.0.0\"\n",
            )
            .file(
                "inner/Cargo.toml",
                "[workspace]\n[workspace.package]\nversion = \"1.2.3\"\n",
            )
            // A package manifest between the member and its workspace.
            .file("inner/group/Cargo.toml", "[package]\nname = \"group\"\n")
            .file(
                "inner/group/member/Cargo.toml",
                "[package]\nname = \"member\"\nversion.workspace = true\n",
            );
        let package = Package::from_manifest(project.path("inner/group/member/Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package.version, "1.2.3");
    }

    #[test]
    fn inheritance_without_a_workspace_leaves_the_field_unset() {
        let project = Project::new();
        project.file(
            "Cargo.toml",
            "[package]\nname = \"lonely\"\nversion.workspace = true\nlicense = { workspace = false }\n",
        );
        let package = Package::from_manifest(project.path("Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package.version, "0.0.0");
        assert_eq!(package.license, None);
    }

    #[test]
    fn a_workspace_without_package_defaults_inherits_nothing() {
        let project = Project::new();
        project
            .file("Cargo.toml", "[workspace]\nmembers = [\"a\"]\n")
            .file(
                "a/Cargo.toml",
                "[package]\nname = \"a\"\nversion.workspace = true\n",
            );
        let package = Package::from_manifest(project.path("a/Cargo.toml"))
            .unwrap()
            .unwrap();
        assert_eq!(package.version, "0.0.0");
    }

    #[test]
    fn virtual_workspaces_have_no_package() {
        let project = Project::new();
        project.file("Cargo.toml", "[workspace]\nmembers = []\n");
        assert_eq!(
            Package::from_manifest(project.path("Cargo.toml")).unwrap(),
            None
        );
    }

    #[test]
    fn a_package_needs_a_name() {
        let project = Project::new();
        project.file("Cargo.toml", "[package]\nversion = \"1.0.0\"\n");
        match Package::from_manifest(project.path("Cargo.toml")) {
            Err(Error::Config { message, .. }) => assert!(message.contains("`name`"), "{message}"),
            other => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[test]
    fn unreadable_and_invalid_manifests_are_errors() {
        let project = Project::new();
        project.file("Cargo.toml", "[package\nname = ");
        assert!(matches!(
            Package::from_manifest(project.path("Cargo.toml")),
            Err(Error::Config { .. })
        ));
        assert!(matches!(
            Package::from_manifest(project.path("missing/Cargo.toml")),
            Err(Error::Config { .. })
        ));
    }
}

mod environment {
    use super::*;

    #[test]
    fn cargo_sets_the_package_variables_for_tests() {
        let package = Package::from_env().expect("cargo sets CARGO_PKG_NAME");
        assert_eq!(package.name, env!("CARGO_PKG_NAME"));
        assert_eq!(package.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(package.license.as_deref(), Some(env!("CARGO_PKG_LICENSE")));
    }

    #[test]
    fn the_build_package_needs_out_dir() {
        // Cargo sets `OUT_DIR` when it runs build scripts, not for test
        // binaries; the check holds either way.
        let expected = std::env::var_os("OUT_DIR").and_then(|_| Package::from_env());
        assert_eq!(Package::from_build_env(), expected);
    }
}

mod root_documents {
    use super::*;

    #[test]
    fn the_minimal_root_has_title_and_version() {
        assert_eq!(
            Package::new("api", "1.0.0").root_document(),
            json!({ "openapi": ROOT_VERSION, "info": { "title": "api", "version": "1.0.0" } })
        );
    }

    #[test]
    fn every_field_is_used() {
        let mut package = Package::new("shop", "2.0.0");
        package.description = Some("A shop".into());
        package.license = Some("MIT".into());
        package.repository = Some("https://example.com/repo".into());
        assert_eq!(
            package.root_document(),
            json!({
                "openapi": ROOT_VERSION,
                "info": {
                    "title": "shop",
                    "version": "2.0.0",
                    "description": "A shop",
                    "license": { "name": "MIT", "identifier": "MIT" },
                },
                "externalDocs": { "url": "https://example.com/repo" },
            })
        );
        package.homepage = Some("https://shop.example.com".into());
        assert_eq!(
            package.root_document()["externalDocs"]["url"],
            "https://shop.example.com",
            "the homepage is preferred"
        );
    }

    #[test]
    fn variables_follow_cargo() {
        let mut package = Package::new("shop", "1.12.3-beta.1+build");
        package.license = Some("MIT".into());
        let variables: std::collections::BTreeMap<_, _> = package.variables().into_iter().collect();
        assert_eq!(variables["CARGO_PKG_NAME"], "shop");
        assert_eq!(variables["CARGO_PKG_VERSION"], "1.12.3-beta.1+build");
        assert_eq!(variables["CARGO_PKG_VERSION_MAJOR"], "1");
        assert_eq!(variables["CARGO_PKG_VERSION_MINOR"], "12");
        assert_eq!(variables["CARGO_PKG_VERSION_PATCH"], "3");
        assert_eq!(variables["CARGO_PKG_LICENSE"], "MIT");
        assert_eq!(
            variables["CARGO_PKG_DESCRIPTION"], "",
            "unset fields are empty"
        );
        assert_eq!(variables.len(), 9);
    }

    #[test]
    fn the_root_version_is_an_openapi_31_version() {
        assert!(ROOT_VERSION.starts_with("3.1."));
    }
}
