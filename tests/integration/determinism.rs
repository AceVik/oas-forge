//! The output depends only on the input: not on the order files are created
//! in, the directory the project lives in, or how often the generator runs.

use crate::common::{Project, ROOT_DOC};
use oas_forge::Generator;

const FILES: &[(&str, &str)] = &[
    ("src/root.rs", ROOT_DOC),
    (
        "src/z_models.rs",
        "/// @openapi\nstruct Zebra { a: String }\n/// @openapi\nstruct Alpha { b: u64 }",
    ),
    (
        "src/a/routes.rs",
        "/// @route GET /b\nfn b() {}\n/// @route GET /a\n/// @return 200: $Zebra\nfn a() {}",
    ),
    (
        "src/m/templates.rs",
        "/// @openapi<T>\nstruct Page<T> { items: Vec<T> }\n/// @route GET /pages\n/// @return 200: $Page<Alpha>\nfn pages() {}",
    ),
    (
        "src/shared.yaml",
        "components:\n  schemas:\n    Money: { type: string }\ntags: [Billing]",
    ),
    (
        "src/extra.json",
        "{\"paths\": {\"/json\": {\"get\": {\"responses\": {}}}}}",
    ),
];

fn generate(order: &[usize]) -> String {
    let project = Project::new();
    for &index in order {
        let (path, content) = FILES[index];
        project.file(path, content);
    }
    Generator::new()
        .input(project.path("src"))
        .output(project.path("out/openapi.yaml"))
        .generate()
        .unwrap();
    project.read("out/openapi.yaml")
}

#[test]
fn file_creation_order_does_not_matter() {
    let forward: Vec<usize> = (0..FILES.len()).collect();
    let reverse: Vec<usize> = forward.iter().rev().copied().collect();
    let shuffled = [3, 0, 5, 1, 4, 2];
    let expected = generate(&forward);
    assert_eq!(generate(&reverse), expected);
    assert_eq!(generate(&shuffled), expected);
}

#[test]
fn output_contains_no_machine_specific_paths() {
    let text = generate(&(0..FILES.len()).collect::<Vec<_>>());
    let temp = std::env::temp_dir();
    assert!(!text.contains(temp.to_string_lossy().as_ref()), "{text}");
}

#[test]
fn repeated_runs_are_identical() {
    let project = Project::new();
    for (path, content) in FILES {
        project.file(path, content);
    }
    let generator = Generator::new()
        .input(project.path("src"))
        .output(project.path("openapi.yaml"))
        .output_fragments(project.path("fragments.json"));
    generator.generate().unwrap();
    let first = (project.read("openapi.yaml"), project.read("fragments.json"));
    generator.generate().unwrap();
    assert_eq!(
        (project.read("openapi.yaml"), project.read("fragments.json")),
        first
    );
}
