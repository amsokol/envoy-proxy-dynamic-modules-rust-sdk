use std::env;
use std::path::PathBuf;

/// Configures the bindgen builder with cross-compilation paths and sysroot settings.
///
/// This function detects if we're cross-compiling and configures the appropriate sysroot
/// paths for the target architecture. It handles both explicit SYSROOT environment variables
/// and auto-detection from Bazel build environments.
pub fn configure_cross_compilation_paths(mut builder: bindgen::Builder) -> bindgen::Builder {
    let target = env::var("TARGET").unwrap();
    let host = env::var("HOST").unwrap();
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
                let out_path = PathBuf::from(&out_dir);
                
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

    builder
}
