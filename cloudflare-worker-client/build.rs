use std::{
    env,
    fs,
    path::PathBuf,
};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let spec_path = manifest_dir.join("openapi/cloudflare-browser-simulator.json");

    println!("cargo:rerun-if-changed={}", spec_path.display());

    let spec =
        serde_json::from_slice::<openapiv3::OpenAPI>(&fs::read(&spec_path).expect("failed to read OpenAPI spec"))
            .expect("failed to parse OpenAPI spec");

    let mut generator = progenitor::Generator::default();
    let tokens = generator
        .generate_tokens(&spec)
        .expect("failed to generate Rust client from OpenAPI spec");
    let ast = syn::parse2(tokens).expect("failed to parse generated Rust client");
    let content = prettyplease::unparse(&ast);

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR")).join("worker_api.rs");
    fs::write(out_path, content).expect("failed to write generated Rust client");
}
