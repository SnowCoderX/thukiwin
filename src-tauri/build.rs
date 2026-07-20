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

/// Explicit Windows app manifest declaring Per-Monitor-V2 DPI awareness.
///
/// This is the real fix for screenshots coming out virtualized/downscaled
/// (e.g. 1920x461 instead of the real ~6000x1440 on a multi-monitor HiDPI
/// desktop). `tauri_build::build()` (the plain, unconfigured call this file
/// used before) still embeds *some* manifest via `embed_resource`, and once
/// a manifest exists at all, Windows refuses to let a runtime call like
/// `SetProcessDpiAwarenessContext` (see main.rs) override whatever DPI
/// awareness that manifest declares (or fails to declare) — the manifest
/// always wins. So the process-level call in `main()` was silently failing;
/// the only reliable fix is declaring the awareness directly in the manifest
/// that actually ships with the exe.
///
/// IMPORTANT: `WindowsAttributes::app_manifest()` *replaces* the entire
/// manifest Tauri would otherwise generate, it does not merge into it. An
/// earlier version of this file supplied only the DPI fragment and dropped
/// the `Microsoft.Windows.Common-Controls` v6 dependency that Tauri's
/// default manifest normally includes — Windows then fell back to the
/// ancient comctl32 v5, which has no `TaskDialogIndirect`, producing an
/// "Entry Point Not Found" error at launch. This version keeps the full
/// standard template (trustInfo, supportedOS compatibility block, and the
/// comctl32 v6 dependency) and adds only the DPI awareness declaration on
/// top of it.
///
/// Both the modern (`dpiAwareness`, Windows 10 1607+) and legacy
/// (`dpiAware`, Vista+) DPI elements are included per Microsoft's own
/// recommendation, so older Windows versions that don't understand the 2016
/// schema still fall back to plain System-DPI-Aware instead of Unaware.
#[cfg(target_os = "windows")]
const WINDOWS_DPI_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity version="1.0.0.0" processorArchitecture="*" name="thuki.app" type="win32"/>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges xmlns="urn:schemas-microsoft-com:asm.v3">
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 / 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
      <!-- Windows 8.1 -->
      <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
      <!-- Windows 8 -->
      <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
      <!-- Windows 7 -->
      <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
      <!-- Windows Vista -->
      <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}"/>
    </application>
  </compatibility>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    </windowsSettings>
  </application>
</assembly>
"#;

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

    #[cfg(target_os = "windows")]
    {
        let attrs = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new().app_manifest(WINDOWS_DPI_MANIFEST),
        );
        tauri_build::try_build(attrs)
            .expect("failed to run tauri_build with custom DPI-aware manifest");
        return;
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build();
}