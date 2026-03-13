fn main() {
    oas_forge::Generator::new()
        .input("src")
        .output_fragments("shared_openapi_fragments.yaml")
        .generate()
        .expect("Failed to generate OpenAPI fragments");
}
