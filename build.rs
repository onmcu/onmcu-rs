// Based on https://github.com/oxidecomputer/progenitor/blob/main/example-build/build.rs
// Copyright 2022 Oxide Computer Company
// Modifications: OnMCU
fn main() {
    let src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("openapi/openapi.json");

    println!("cargo:rerun-if-changed={}", src.display());
    let file = std::fs::File::open(src).unwrap();
    let spec = serde_json::from_reader(file).unwrap();

    let mut settings = progenitor::GenerationSettings::new();
    settings.with_interface(progenitor::InterfaceStyle::Builder);
    let mut generator = progenitor::Generator::new(&settings);

    let tokens = generator.generate_tokens(&spec).unwrap();
    let ast = syn::parse2(tokens).unwrap();
    let content = prettyplease::unparse(&ast);

    // Generate only typespace because the client currently doesn't support setting (auth-)headers
    // per request, only once for the entire reqwest client.
    let type_space = generator.get_type_space();
    let _contents =
        prettyplease::unparse(&syn::parse2::<syn::File>(type_space.to_stream()).unwrap());

    let mut out_file = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).to_path_buf();
    out_file.push("codegen.rs");

    std::fs::write(out_file, content).unwrap();
}
