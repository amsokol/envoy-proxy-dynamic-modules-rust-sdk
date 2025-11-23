use std::env;
use std::path::PathBuf;

fn main() {
    // This is Envoy CI specific: Check if "/opt/llvm/bin/clang" exists, and if it does, set the
    // CLANG_PATH environment variable. CLANG_PATH is for clang-sys used by bindgen:
    // https://github.com/KyleMayes/clang-sys?tab=readme-ov-file#environment-variables
    //
    // "/opt/llvm/bin/clang" exists in Envoy CI containers. If the clang doesn't exist there, bindgen
    // will try to use the system clang from PATH. So, this doesn't affect the local builds.
    // In any case, clang must be found to build the bindings.
    //
    // TODO: add /opt/llvm/bin to PATH in the CI containers. That would be a better solution.
    if std::fs::metadata("/opt/llvm/bin/clang").is_ok() {
        env::set_var("CLANG_PATH", "/opt/llvm/bin/clang");
    }

    println!("cargo:rerun-if-changed=abi.h");
    println!("cargo:rerun-if-env-changed=SYSROOT");
    println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();
    
    let mut builder = bindgen::Builder::default()
        .header("abi.h")
        .header("abi_version.h")
        .clang_arg("-v")
        .default_enum_style(bindgen::EnumVariation::Rust {
            non_exhaustive: false,
        })
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .parse_callbacks(Box::new(TrimEnumNameFromVariantName));

    // Detect cross-compilation and configure sysroot
    let is_cross_compiling = target != host;
    
    // First, check if SYSROOT is explicitly set
    let sysroot_opt = env::var("SYSROOT").ok().or_else(|| {
        // If not explicitly set and we are cross-compiling, try to auto-detect from Bazel
        if is_cross_compiling {
            // Determine the sysroot name based on target
            // Bazel uses +_repo_rules+ prefix for external repositories
            let sysroot_name = if target.contains("aarch64") || target.contains("arm64") {
                "+_repo_rules+sysroot_linux_arm64"
            } else if target.contains("x86_64") {
                "+_repo_rules+sysroot_linux_amd64"
            } else {
                return None;
            };
            
            // In Bazel, OUT_DIR is typically in:
            // bazel-out/<config>/bin/external/<package>/...
            // We need to find the sysroot in the external directory which could be at various levels
            if let Ok(out_dir) = env::var("OUT_DIR") {
                eprintln!("DEBUG: OUT_DIR = {}", out_dir);
                let  out_path = PathBuf::from(&out_dir);
                
                // Navigate up from OUT_DIR looking for common Bazel directory markers
                let mut current = out_path.as_path();
                for _ in 0..15 {  // Limit depth to avoid infinite loop
                    if let Some(parent) = current.parent() {
                        // Check for external directory at this level
                        let external_dir = parent.join("external");
                        let sysroot_path = external_dir.join(sysroot_name);
                        
                        eprintln!("DEBUG: Checking {}", sysroot_path.display());
                        if sysroot_path.exists() {
                            eprintln!("Auto-detected sysroot at: {}", sysroot_path.display());
                            return Some(sysroot_path.to_string_lossy().to_string());
                        }
                        
                        // Also check if current directory name contains "bazel" - might be output_base
                        if let Some(dir_name) = parent.file_name().and_then(|n| n.to_str()) {
                            if dir_name.contains("bazel") || dir_name.starts_with("_bazel_") {
                                // Try external directly under this directory
                                let external_here = parent.join("external");
                                let sysroot_here = external_here.join(sysroot_name);
                                eprintln!("DEBUG: Checking bazel dir {}", sysroot_here.display());
                                if sysroot_here.exists() {
                                    eprintln!("Auto-detected sysroot at: {}", sysroot_here.display());
                                    return Some(sysroot_here.to_string_lossy().to_string());
                                }
                            }
                        }
                        
                        current = parent;
                    } else {
                        break;
                    }
                }
                
                eprintln!("DEBUG: Sysroot '{}' not found in any external directory", sysroot_name);
            }
        }
        None
    });

    // Add sysroot configuration if available
    if let Some(sysroot) = sysroot_opt {
        eprintln!("Using sysroot: {}", sysroot);
        builder = builder.clang_arg(format!("--sysroot={}", sysroot));
        
        // Add target-specific include paths
        if target.contains("aarch64") || target.contains("arm64") {
            builder = builder
                .clang_arg(format!("-I{}/usr/include/aarch64-linux-gnu", sysroot))
                .clang_arg(format!("-I{}/usr/include", sysroot));
        } else if target.contains("x86_64") {
            builder = builder
                .clang_arg(format!("-I{}/usr/include/x86_64-linux-gnu", sysroot))
                .clang_arg(format!("-I{}/usr/include", sysroot));
        }
    } else if is_cross_compiling {
        eprintln!("WARNING: Cross-compiling but no sysroot found. Build may fail.");
        eprintln!("  TARGET: {}", target);
        eprintln!("  HOST: {}", host);
    }

    // Support additional clang args via environment variable
    if let Ok(extra_args) = env::var("BINDGEN_EXTRA_CLANG_ARGS") {
        eprintln!("Using extra bindgen clang args: {}", extra_args);
        for arg in extra_args.split_whitespace() {
            builder = builder.clang_arg(arg);
        }
    }

    let bindings = builder
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings");
}

#[derive(Debug)]
// This allows us to simplify the enum variant names.
// Otherwise, the generated enum result would be `EnumName::EnumName_VariantName`. E.g.
// `envoy_dynamic_module_type_on_http_filter_response_trailers_status::envoy_dynamic_module_type_on_http_filter_response_trailers_status_Continue`
// instead of `envoy_dynamic_module_type_on_http_filter_response_trailers_status::Continue`.
//
// See https://github.com/rust-lang/rust-bindgen/issues/777
struct TrimEnumNameFromVariantName;

impl bindgen::callbacks::ParseCallbacks for TrimEnumNameFromVariantName {
    fn enum_variant_name(
        &self,
        enum_name: Option<&str>,
        original_variant_name: &str,
        _variant_value: bindgen::callbacks::EnumVariantValue,
    ) -> Option<String> {
        let variant_name = match enum_name {
            Some(enum_name) => original_variant_name
                .trim_start_matches(enum_name)
                .trim_start_matches('_'),
            None => original_variant_name,
        };
        Some(variant_name.to_string())
    }
}
