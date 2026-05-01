// Based on https://github.com/oxidecomputer/progenitor/blob/main/example-build/build.rs
// Copyright 2022 Oxide Computer Company
// Modifications: OnMCU
fn main() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi/openapi.json");

    println!("cargo:rerun-if-changed={}", src.display());
    let file = std::fs::File::open(&src).expect(
        "missing openapi/openapi.json — sync it from the controller release \
         (.github/workflows/openapi-sync.yml) or check it in manually",
    );
    let spec = serde_json::from_reader(file).expect("openapi/openapi.json is not valid JSON");

    let mut settings = progenitor::GenerationSettings::new();
    settings.with_interface(progenitor::InterfaceStyle::Builder);
    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator
        .generate_tokens(&spec)
        .expect("progenitor failed to generate client tokens from the openapi spec");
    let ast = syn::parse2(tokens).expect("progenitor produced tokens that did not parse as Rust");
    let content = prettyplease::unparse(&ast);

    // Generate only typespace because the client currently doesn't support setting (auth-)headers
    // per request, only once for the entire reqwest client.
    let type_space = generator.get_type_space();
    let _contents = prettyplease::unparse(
        &syn::parse2::<syn::File>(type_space.to_stream())
            .expect("progenitor produced typespace tokens that did not parse as Rust"),
    );

    let out_dir = std::env::var("OUT_DIR").expect("cargo did not set OUT_DIR");
    let mut out_file = std::path::Path::new(&out_dir).to_path_buf();
    out_file.push("codegen.rs");

    std::fs::write(&out_file, content)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_file.display()));
}
