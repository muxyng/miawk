use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set");
    let cargo_toml_path = Path::new(&manifest_dir).join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&cargo_toml_path).expect("read Cargo.toml");
    let codex_version = read_codex_version(&cargo_toml).expect("package.metadata.codex.version");

    println!("cargo:rustc-env=RSC_CODEX_VERSION={codex_version}");
}

fn read_codex_version(cargo_toml: &str) -> Option<String> {
    let mut in_codex_section = false;

    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_codex_section = trimmed == "[package.metadata.codex]";
            continue;
        }

        if !in_codex_section || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() != "version" {
            continue;
        }

        return Some(value.trim().trim_matches('"').to_string());
    }

    None
}
