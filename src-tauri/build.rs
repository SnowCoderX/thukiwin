use std::collections::HashMap;
use std::path::{Path, PathBuf};

const BACKEND_ENV_KEYS: &[&str] = &["THUKI_SYSTEM_PROMPT", "THUKI_SUPPORTED_AI_MODELS"];

fn load_env_file(path: &Path, vars: &mut HashMap<String, String>) {
    if let Ok(iter) = dotenvy::from_path_iter(path) {
        for item in iter.flatten() {
            vars.insert(item.0, item.1);
        }
    }
}

fn candidate_env_files() -> Vec<PathBuf> {
    let mut files = vec![PathBuf::from("../.env")];
    files.push(PathBuf::from("../.env.local"));

    let profile = std::env::var("PROFILE").unwrap_or_default();
    let mode = if profile.eq_ignore_ascii_case("release") {
        "production"
    } else {
        "development"
    };

    files.push(PathBuf::from(format!("../.env.{mode}")));
    files.push(PathBuf::from(format!("../.env.{mode}.local")));
    files
}

fn main() {
    // Register cfg flags set by cargo-llvm-cov so rustc doesn't warn about unknown cfgs.
    println!("cargo::rustc-check-cfg=cfg(coverage)");
    println!("cargo::rustc-check-cfg=cfg(coverage_nightly)");

    let mut embedded_vars = HashMap::new();
    for path in candidate_env_files() {
        println!("cargo::rerun-if-changed={}", path.display());
        load_env_file(&path, &mut embedded_vars);
    }

    for key in BACKEND_ENV_KEYS {
        if let Ok(value) = std::env::var(key) {
            embedded_vars.insert((*key).to_string(), value);
        }
    }

    for key in BACKEND_ENV_KEYS {
        if let Some(value) = embedded_vars.get(*key) {
            println!("cargo::rustc-env={key}={value}");
        }
    }

    tauri_build::build()
}
