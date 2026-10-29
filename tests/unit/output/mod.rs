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

    #[cfg(unix)]
    #[test]
    fn preexisting_temporary_symlinks_cannot_overwrite_other_files() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("victim.txt", "keep me");
        let temp = project.path(&format!(".api.json.{}.tmp", std::process::id()));
        std::os::unix::fs::symlink(project.path("victim.txt"), &temp).unwrap();

        Generator::new()
            .input(project.path("src"))
            .output(project.path("api.json"))
            .generate()
            .unwrap();

        assert_eq!(project.read("victim.txt"), "keep me");
        assert!(std::fs::symlink_metadata(temp).unwrap().is_symlink());
        assert!(
            !std::fs::symlink_metadata(project.path("api.json"))
                .unwrap()
                .is_symlink()
        );
        let document: serde_json::Value = serde_json::from_str(&project.read("api.json")).unwrap();
        assert_eq!(document["info"]["title"], "Test API");
    }

    #[test]
    fn failed_replacement_leaves_no_temporary_file() {
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("out/api.json/keep.txt", "keep me");
        let result = Generator::new()
            .input(project.path("src"))
            .output(project.path("out/api.json"))
            .generate();
        assert!(matches!(result, Err(Error::Io { .. })), "{result:?}");
        assert_eq!(project.read("out/api.json/keep.txt"), "keep me");
        assert_eq!(std::fs::read_dir(project.path("out")).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn rewriting_an_output_does_not_widen_its_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("api.json", "old");
        std::fs::set_permissions(
            project.path("api.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        Generator::new()
            .input(project.path("src"))
            .output(project.path("api.json"))
            .generate()
            .unwrap();
        assert_eq!(
            std::fs::metadata(project.path("api.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_destination_symlink_keeps_its_target_and_normal_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("target.txt", "keep me")
            .file("ordinary.txt", "normal");
        std::os::unix::fs::symlink(project.path("target.txt"), project.path("api.json")).unwrap();
        Generator::new()
            .input(project.path("src"))
            .output(project.path("api.json"))
            .generate()
            .unwrap();
        assert_eq!(project.read("target.txt"), "keep me");
        let actual = std::fs::symlink_metadata(project.path("api.json")).unwrap();
        assert!(actual.is_file());
        let expected = std::fs::metadata(project.path("ordinary.txt")).unwrap();
        assert_eq!(
            actual.permissions().mode() & 0o777,
            expected.permissions().mode() & 0o777
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_files_use_the_output_directory_not_the_working_directory() {
        use std::os::unix::fs::PermissionsExt;

        if let Some(root) = std::env::var_os("OAS_FORGE_OUTPUT_DIRECTORY_TEST") {
            let root = std::path::PathBuf::from(root);
            Generator::new()
                .input(root.join("src"))
                .output(root.join("out/api.json"))
                .generate()
                .unwrap();
            return;
        }
        let project = Project::new();
        project
            .file("src/root.rs", ROOT_DOC)
            .file("locked/keep.txt", "keep me");
        let locked = project.path("locked");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "output::writing::temporary_files_use_the_output_directory_not_the_working_directory"])
            .current_dir(&locked)
            .env("OAS_FORGE_OUTPUT_DIRECTORY_TEST", project.path("."))
            .output();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = result.unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(project.path("out/api.json").is_file());
        assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_writers_publish_complete_documents() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let spec = Generator::new()
            .input(project.path("src"))
            .output(project.path("out/api.json"))
            .build()
            .unwrap();
        let barrier = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    barrier.wait();
                    spec.write().unwrap();
                });
            }
        });
        let actual: serde_json::Value =
            serde_json::from_str(&project.read("out/api.json")).unwrap();
        assert_eq!(actual, spec.render(OutputKind::Full).unwrap());
        assert_eq!(std::fs::read_dir(project.path("out")).unwrap().count(), 1);
    }

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

    /// An output path that leads to anything but a regular file is outdated
    /// without being read: a FIFO would block, `/dev/zero` never ends.
    #[cfg(unix)]
    #[test]
    fn outputs_that_are_not_regular_files_are_replaced_unread() {
        use std::os::unix::fs::{FileTypeExt, symlink};

        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let fifo = project.path("fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        // The FIFO comes first: before the fix, it blocks instead of
        // letting `/dev/zero` fill the memory.
        let outputs = [project.path("api.json"), project.path("api.yaml")];
        symlink(&fifo, &outputs[0]).unwrap();
        symlink("/dev/zero", &outputs[1]).unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        let (input, paths) = (project.path("src"), outputs.clone());
        std::thread::spawn(move || {
            let spec = Generator::new()
                .input(input)
                .output(&paths[0])
                .output(&paths[1])
                .build()
                .unwrap();
            let outdated = spec.check().unwrap();
            spec.write().unwrap();
            sender.send(outdated).unwrap();
        });
        let outdated = receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("checking and writing finish");
        assert_eq!(outdated, outputs);
        for output in &outputs {
            assert!(std::fs::symlink_metadata(output).unwrap().is_file());
        }
        assert!(
            std::fs::symlink_metadata(&fifo)
                .unwrap()
                .file_type()
                .is_fifo()
        );
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

    /// Only as much of an output is read as a current one can have; text
    /// after an equivalent document still makes it outdated.
    #[test]
    fn text_after_an_equivalent_document_makes_it_outdated() {
        let project = Project::new();
        project.file("src/root.rs", ROOT_DOC);
        let generator = Generator::new()
            .input(project.path("src"))
            .output(project.path("api.yaml"));
        generator.generate().unwrap();
        let crlf = project.read("api.yaml").replace('\n', "\r\n");
        project.file("api.yaml", &format!("{crlf}x"));
        assert_eq!(
            generator.build().unwrap().check().unwrap(),
            [project.path("api.yaml")]
        );
    }
}
