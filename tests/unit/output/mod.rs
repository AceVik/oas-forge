//! `src/output/`: formats, output strategies, exports and writing.

mod asyncapi;
mod json_schema;
mod openapi30;
mod swagger2;

use crate::common::{Project, ROOT_DOC, yaml};
use oas_forge::error::Error;
use oas_forge::output::{Format, OutputKind};
use oas_forge::{Generator, Spec};
use serde_json::json;
use std::path::Path;

fn spec(project: &Project) -> Spec {
    Generator::new()
        .input(project.path("src"))
        .build()
        .expect("build succeeds")
}

mod format {
    use super::*;

    #[test]
    fn is_chosen_by_extension() {
        assert_eq!(Format::of(Path::new("api.json")), Format::Json);
        for path in ["api.yaml", "api.yml", "api", "api.JSON", "dir.json/api.txt"] {
            assert_eq!(Format::of(Path::new(path)), Format::Yaml, "{path}");
        }
    }

    #[test]
    fn json_is_pretty_with_a_trailing_newline() {
        assert_eq!(
            Format::Json.render(&json!({ "a": [1] })),
            "{\n  \"a\": [\n    1\n  ]\n}\n"
        );
    }

    #[test]
    fn yaml_has_no_document_marker_and_round_trips() {
        let value = json!({ "openapi": "3.1.0", "paths": { "/a": { "get": { "x": "yes" } } } });
        let text = Format::Yaml.render(&value);
        assert!(text.starts_with("openapi: "), "{text}");
        assert!(!text.starts_with("---"), "{text}");
        assert!(text.ends_with('\n'));
        assert_eq!(yaml(&text), value, "strings like `yes` stay strings");
    }
}

mod strategies {
    use super::*;

    fn project() -> Project {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC).file(
            "src/api.rs",
            r#"
            /// @openapi
            /// tags: [{ name: B }]
            /// servers: [{ url: "https://x" }]
            /// components:
            ///   securitySchemes: { b: { type: http, scheme: bearer }, a: { type: http, scheme: basic } }
            /// paths:
            ///   /a: { get: { responses: { "200": { description: OK } } } }
            fn api() {}

            /// @openapi
            struct Zebra { a: String }
            /// @openapi
            struct Alpha { a: String }
            "#,
        );
        project
    }

    #[test]
    fn full_puts_root_keys_in_specification_order_and_sorts_components() {
        let full = spec(&project()).render(OutputKind::Full).unwrap();
        let keys: Vec<_> = full.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            ["openapi", "info", "servers", "paths", "components", "tags"]
        );
        let schemas: Vec<_> = full["components"]["schemas"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(schemas, ["Alpha", "Zebra"]);
        let schemes: Vec<_> = full["components"]["securitySchemes"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(schemes, ["a", "b"]);
    }

    #[test]
    fn full_requires_a_root() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @openapi\nstruct A { a: String }");
        assert!(matches!(
            spec(&project).render(OutputKind::Full),
            Err(Error::NoRootFound)
        ));
    }

    #[test]
    fn schemas_and_paths_are_sub_maps() {
        let spec = spec(&project());
        let schemas = spec.render(OutputKind::Schemas).unwrap();
        assert!(schemas.get("Alpha").is_some() && schemas.get("openapi").is_none());
        let paths = spec.render(OutputKind::Paths).unwrap();
        assert!(paths.get("/a").is_some());
    }

    #[test]
    fn missing_sections_render_as_empty_maps() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\n/// info: { title: T, version: '1' }\nfn f() {}",
        );
        let spec = spec(&project);
        assert_eq!(spec.render(OutputKind::Schemas).unwrap(), json!({}));
        assert_eq!(spec.render(OutputKind::Paths).unwrap(), json!({}));
    }

    #[test]
    fn fragments_drop_the_root_and_carry_templates() {
        let project = project();
        project.file(
            "src/templates.rs",
            "/// @openapi-fragment Ok(what)\n/// description: {{what}}\nfn f() {}\n\n/// @openapi<T>\nstruct Page<T> { items: Vec<T> }",
        );
        let fragments = spec(&project).render(OutputKind::Fragments).unwrap();
        for key in ["openapi", "info", "servers"] {
            assert!(fragments.get(key).is_none(), "{key}");
        }
        let components = &fragments["components"];
        assert_eq!(components["x-oas-forge-format"], 1);
        assert_eq!(
            components["x-oas-forge-templates"]["Page"]["params"],
            json!(["T"])
        );
        assert_eq!(
            components["x-oas-forge-fragments"]["Ok"]["params"],
            json!(["what"])
        );
    }

    #[test]
    fn fragments_without_templates_have_no_transport_keys() {
        let fragments = spec(&project()).render(OutputKind::Fragments).unwrap();
        let components = fragments["components"].as_object().unwrap();
        assert!(components.keys().all(|k| !k.starts_with("x-oas-forge")));
    }

    #[test]
    fn fragments_add_components_when_needed() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @openapi-fragment F\n/// a: 1\nfn f() {}");
        let fragments = spec(&project).render(OutputKind::Fragments).unwrap();
        assert!(
            fragments["components"]["x-oas-forge-fragments"]
                .get("F")
                .is_some()
        );
    }
}

mod writing {
    use super::*;

    #[test]
    fn writes_every_output_in_its_format_and_creates_directories() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let written = Generator::new()
            .input(project.path("src"))
            .output(project.path("out/deep/api.json"))
            .output_paths(project.path("out/paths.yaml"))
            .build()
            .unwrap()
            .write()
            .unwrap();
        assert_eq!(
            written,
            [
                project.path("out/deep/api.json"),
                project.path("out/paths.yaml")
            ]
        );
        assert!(
            project
                .read("out/deep/api.json")
                .starts_with("{\n  \"openapi\"")
        );
        assert_eq!(project.read("out/paths.yaml"), "{}\n");
        let leftovers: Vec<_> = std::fs::read_dir(project.path("out"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temporary files remain");
    }

    #[test]
    fn nothing_is_written_when_one_output_cannot_be_rendered() {
        let project = Project::new();
        project.file("src/lib.rs", "/// @openapi\nstruct A { a: String }");
        let result = Generator::new()
            .input(project.path("src"))
            .output_schemas(project.path("schemas.yaml"))
            .output(project.path("api.yaml"))
            .build()
            .unwrap()
            .write();
        assert!(matches!(result, Err(Error::NoRootFound)));
        assert!(!project.path("schemas.yaml").exists());
    }

    #[test]
    fn a_path_without_file_name_is_an_io_error() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let result = Generator::new()
            .input(project.path("src"))
            .output(project.path("out/.."))
            .build()
            .unwrap()
            .write();
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
    }

    #[cfg(unix)]
    #[test]
    fn unchanged_files_are_not_rewritten() {
        use std::os::unix::fs::PermissionsExt;

        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("out/api.yaml"));
        generator.generate().unwrap();

        // A read-only directory makes every write attempt fail.
        let out = project.path("out");
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o555)).unwrap();
        let unchanged = generator.generate();
        project.file("src/extra.rs", "/// @openapi\nstruct New { a: String }");
        let changed = generator.generate();
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(unchanged.is_ok(), "{unchanged:?}");
        assert!(matches!(changed, Err(Error::Io { .. })), "{changed:?}");
    }
}

mod review_fixes {
    use super::*;
    use crate::common::spec as official;

    #[test]
    fn schemas_output_is_sorted_like_the_components() {
        let project = Project::new();
        project.file(
            "src/lib.rs",
            "/// @openapi\nstruct Zebra { a: u8 }\n/// @openapi\nstruct Alpha { b: u8 }",
        );
        let schemas = spec(&project).render(OutputKind::Schemas).unwrap();
        let names: Vec<_> = schemas.as_object().unwrap().keys().cloned().collect();
        assert_eq!(names, ["Alpha", "Zebra"]);
    }

    #[test]
    fn full_documents_get_the_paths_their_version_requires() {
        for (root, extra, expected) in [
            ("3.0.3", "/// @openapi\nstruct A { a: u8 }", Some(json!({}))),
            ("3.1.0", "", Some(json!({}))),
            ("3.2.0", "", Some(json!({}))),
            ("3.1.0", "/// @openapi\nstruct A { a: u8 }", None),
        ] {
            let project = Project::new();
            project.file(
                "src/lib.rs",
                &format!(
                    "/// @openapi\n/// openapi: {root}\n/// info: {{ title: T, version: '1' }}\nfn root() {{}}\n{extra}"
                ),
            );
            let full = spec(&project).render(OutputKind::Full).unwrap();
            assert_eq!(full.get("paths").cloned(), expected, "{root} {extra}");
            let errors = official::validate(&full);
            assert!(errors.is_empty(), "{root}: {errors:?}\n{full:#}");
        }
    }

    #[test]
    fn line_endings_do_not_make_an_output_outdated() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"))
            .output(project.path("api.json"));
        generator.generate().unwrap();
        for file in ["api.yaml", "api.json"] {
            let crlf = project.read(file).replace('\n', "\r\n");
            project.file(file, &crlf);
        }
        let spec = generator.build().unwrap();
        assert!(spec.check().unwrap().is_empty());
        spec.write().unwrap();
        assert!(
            project.read("api.yaml").contains("\r\n"),
            "an equivalent file is not rewritten"
        );

        project.file(
            "api.yaml",
            &project.read("api.yaml").replace("Test API", "Old"),
        );
        assert_eq!(spec.check().unwrap(), [project.path("api.yaml")]);
    }
}
