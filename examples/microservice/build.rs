fn main() {
    oas_forge::Generator::new()
        .input("src")
        .include("../shared-models/shared_openapi_fragments.yaml")
        .output("openapi.yaml")
        .generate()
        .expect("Failed to generate OpenAPI spec");
}
