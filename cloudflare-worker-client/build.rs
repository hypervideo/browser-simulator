use std::{
    env,
    fs,
    path::PathBuf,
};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let spec_path = manifest_dir.join("../cloudflare-browser-simulator/cli/openapi/cloudflare-browser-simulator.json");

    println!("cargo:rerun-if-changed={}", spec_path.display());

    let spec_bytes = fs::read(&spec_path).unwrap_or_else(|error| {
        panic!(
            "failed to read OpenAPI spec at {}: {error}. Initialize the cloudflare-browser-simulator submodule with `git submodule update --init --recursive`.",
            spec_path.display()
        )
    });
    let mut spec = serde_json::from_slice::<openapiv3::OpenAPI>(&spec_bytes).expect("failed to parse OpenAPI spec");
    preserve_create_browser_log_presence(&mut spec);

    let mut generator = progenitor::Generator::default();
    let tokens = generator
        .generate_tokens(&spec)
        .expect("failed to generate Rust client from OpenAPI spec");
    let ast = syn::parse2(tokens).expect("failed to parse generated Rust client");
    let content = prettyplease::unparse(&ast);

    let out_path = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR")).join("worker_api.rs");
    fs::write(out_path, content).expect("failed to write generated Rust client");
}

fn preserve_create_browser_log_presence(spec: &mut openapiv3::OpenAPI) {
    let components = spec.components.as_mut().expect("OpenAPI spec has no components");
    let response = components
        .schemas
        .get_mut("SessionCreateResponse")
        .expect("OpenAPI spec has no SessionCreateResponse");
    let openapiv3::ReferenceOr::Item(response) = response else {
        panic!("SessionCreateResponse must be an inline schema");
    };
    let openapiv3::SchemaKind::Type(openapiv3::Type::Object(response)) = &mut response.schema_kind else {
        panic!("SessionCreateResponse must be an object schema");
    };
    let browser_log = response
        .properties
        .get_mut("browserLog")
        .expect("SessionCreateResponse has no browserLog field");
    let openapiv3::ReferenceOr::Item(browser_log) = browser_log else {
        panic!("SessionCreateResponse.browserLog must be an inline schema");
    };

    // Progenitor maps an optional array to an empty Vec. Nullable keeps its
    // presence as Option<Vec<_>>, so an older worker remains detectable.
    browser_log.schema_data.nullable = true;
}
