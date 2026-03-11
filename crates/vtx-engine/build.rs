//! Build script for vtx-engine
//!
//! Downloads prebuilt whisper.cpp binaries for the target platform.
//!
//! On Windows x64: downloads BOTH CUDA and CPU-only prebuilt binaries into
//! separate `cuda/` and `cpu/` subdirectories so the runtime can try CUDA
//! first and fall back to CPU transparently.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const WHISPER_VERSION: &str = "1.8.2";
const GITHUB_RELEASE_BASE: &str = "https://github.com/ggml-org/whisper.cpp/releases/download";

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let cuda_enabled = env::var("CARGO_FEATURE_CUDA").is_ok();

    // macOS: Link ScreenCaptureKit framework for system audio capture
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=ScreenCaptureKit");
        println!("cargo:rustc-link-lib=framework=CoreMedia");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
    }

    // Linux: Build whisper.cpp from source using CMake
    if target_os == "linux" {
        build_whisper_linux(cuda_enabled);
        return;
    }

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Use a stable cache directory in target/ rather than OUT_DIR
    let stable_cache_dir = out_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "target").unwrap_or(false))
        .map(|p| p.join("whisper-cache"))
        .unwrap_or_else(|| out_dir.join("whisper-cache"));

    // Windows x64: download BOTH CUDA and CPU-only prebuilt binaries.
    // They are placed in separate subdirectories (cuda/ and cpu/) so the app
    // can try the CUDA variant first and fall back to CPU at runtime.
    // The `cuda` feature flag has no effect on Windows (it is Linux-only).
    if target_os == "windows" && target_arch == "x86_64" {
        if cuda_enabled {
            println!("cargo:warning=Note: --features cuda has no effect on Windows (GPU+CPU support is always included)");
        }
        download_windows_x64_dual_binaries(&stable_cache_dir, &out_dir);
        println!("cargo:rerun-if-changed=build.rs");
        return;
    }

    let (zip_name, lib_names): (String, Vec<&str>) = match (
        target_os.as_str(),
        target_arch.as_str(),
    ) {
        ("macos", _) => {
            if cuda_enabled {
                println!("cargo:warning=CUDA feature has no effect on macOS - using Metal acceleration via prebuilt framework");
            }
            println!(
                "cargo:warning=Downloading whisper.cpp v{} for macOS (Metal-enabled)",
                WHISPER_VERSION
            );
            (
                format!("whisper-v{}-xcframework.zip", WHISPER_VERSION),
                vec!["libwhisper.dylib"],
            )
        }
        _ => {
            println!(
                "cargo:warning=No prebuilt whisper.cpp binary for {}/{}",
                target_os, target_arch
            );
            return;
        }
    };

    let download_url = format!("{}/v{}/{}", GITHUB_RELEASE_BASE, WHISPER_VERSION, zip_name);

    // Check if already cached
    let all_cached = lib_names
        .iter()
        .all(|name| stable_cache_dir.join(name).exists());

    if !all_cached {
        fs::create_dir_all(&stable_cache_dir).expect("Failed to create cache directory");

        println!(
            "cargo:warning=Downloading whisper.cpp from {}",
            download_url
        );

        let response = reqwest::blocking::get(&download_url)
            .unwrap_or_else(|e| panic!("Failed to download whisper.cpp: {}", e));

        if !response.status().is_success() {
            panic!("Failed to download whisper.cpp: HTTP {}", response.status());
        }

        let bytes = response
            .bytes()
            .expect("Failed to read whisper.cpp download");

        let cursor = io::Cursor::new(&bytes);
        let mut archive = zip::ZipArchive::new(cursor).expect("Failed to open whisper.cpp zip");

        for lib_name in &lib_names {
            let dest_path = stable_cache_dir.join(lib_name);

            // Search for the file in the archive (may be in a subdirectory).
            //
            // The macOS xcframework zip ships the dylib inside a .framework bundle
            // at a path like:
            //   build-apple/whisper.xcframework/macos-arm64_x86_64/
            //     whisper.framework/Versions/A/whisper
            //
            // That entry has no .dylib extension, so we also match it explicitly
            // and save it as `libwhisper.dylib`.
            let mut found = false;
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).expect("Failed to read zip entry");
                let name = file.name().to_string();

                // Match either:
                //   - a zip entry whose name ends with the target lib name, OR
                //   - the macOS framework binary (no extension) inside the macos slice
                let is_match = (!name.contains("__MACOSX"))
                    && (name.ends_with(lib_name)
                        || (lib_name == &"libwhisper.dylib"
                            && name.contains("macos")
                            && name.ends_with("/whisper.framework/Versions/A/whisper")));

                if is_match {
                    let mut dest = fs::File::create(&dest_path)
                        .unwrap_or_else(|e| panic!("Failed to create {}: {}", lib_name, e));
                    io::copy(&mut file, &mut dest)
                        .unwrap_or_else(|e| panic!("Failed to extract {}: {}", lib_name, e));
                    found = true;
                    println!(
                        "cargo:warning=Extracted {} from {} ({} bytes)",
                        lib_name,
                        name,
                        dest_path.metadata().map(|m| m.len()).unwrap_or(0)
                    );
                    break;
                }
            }

            if !found {
                println!(
                    "cargo:warning={} not found in archive (non-fatal)",
                    lib_name
                );
            }
        }
    } else {
        println!("cargo:warning=Using cached whisper.cpp binaries");
    }

    // Copy to output directory
    for lib_name in &lib_names {
        let src = stable_cache_dir.join(lib_name);
        if src.exists() {
            let dest = out_dir.join(lib_name);
            if let Err(e) = fs::copy(&src, &dest) {
                println!(
                    "cargo:warning=Failed to copy {} to OUT_DIR: {}",
                    lib_name, e
                );
            }
        }
    }

    // Also copy to target/release or target/debug for runtime
    if let Some(target_dir) = out_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "target").unwrap_or(false))
    {
        let profile = if env::var("PROFILE").unwrap_or_default() == "release" {
            "release"
        } else {
            "debug"
        };
        let profile_dir = target_dir.join(profile);
        if profile_dir.exists() {
            for lib_name in &lib_names {
                let src = stable_cache_dir.join(lib_name);
                if src.exists() {
                    let dest = profile_dir.join(lib_name);
                    if let Err(e) = fs::copy(&src, &dest) {
                        println!(
                            "cargo:warning=Failed to copy {} to {}: {}",
                            lib_name, profile, e
                        );
                    }
                }
            }
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}

/// Download both CUDA and CPU-only prebuilt binaries for Windows x64.
///
/// The CUDA variant includes GPU acceleration via ggml-cuda.dll + CUDA runtime,
/// but requires NVIDIA drivers (nvcuda.dll) to load. The CPU variant works on
/// any machine. Both are placed in separate subdirectories so the app can try
/// CUDA first and fall back to CPU at runtime.
///
/// Layout under the profile output directory (picked up by Tauri bundling):
///   cuda/  - CUDA-enabled DLLs (ggml.dll links to ggml-cuda.dll → nvcuda.dll)
///   cpu/   - CPU-only DLLs (no CUDA dependency)
fn download_windows_x64_dual_binaries(stable_cache_dir: &Path, out_dir: &Path) {
    fs::create_dir_all(stable_cache_dir).expect("Failed to create cache directory");

    let target_dir = out_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "target").unwrap_or(false))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| out_dir.join("..").join("..").join(".."));

    let cuda_libs: Vec<&str> = vec![
        "whisper.dll",
        "ggml.dll",
        "ggml-base.dll",
        "ggml-cpu.dll",
        "ggml-cuda.dll",
        "cublas64_12.dll",
        "cublasLt64_12.dll",
        "cudart64_12.dll",
    ];

    let cpu_libs: Vec<&str> = vec!["whisper.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll"];

    // Download and extract CUDA variant
    let cuda_cache = stable_cache_dir.join(format!("whisper-{}-x86_64-cuda-lib", WHISPER_VERSION));
    let cuda_zip = stable_cache_dir.join(format!("whisper-{}-x86_64-cuda.zip", WHISPER_VERSION));
    fs::create_dir_all(&cuda_cache).expect("Failed to create CUDA cache directory");

    if !cuda_cache.join("whisper.dll").exists() {
        if !cuda_zip.exists() {
            let url = format!(
                "{}/v{}/whisper-cublas-12.4.0-bin-x64.zip",
                GITHUB_RELEASE_BASE, WHISPER_VERSION
            );
            println!(
                "cargo:warning=Downloading CUDA whisper.cpp binaries from: {}",
                url
            );
            download_file(&url, &cuda_zip).expect("Failed to download CUDA whisper.cpp binary");
        }
        println!("cargo:warning=Extracting CUDA whisper.cpp libraries...");
        extract_zip_libraries(&cuda_zip, &cuda_cache, &cuda_libs)
            .expect("Failed to extract CUDA whisper.cpp libraries");
    }

    // Download and extract CPU variant
    let cpu_cache = stable_cache_dir.join(format!("whisper-{}-x86_64-cpu-lib", WHISPER_VERSION));
    let cpu_zip = stable_cache_dir.join(format!("whisper-{}-x86_64-cpu.zip", WHISPER_VERSION));
    fs::create_dir_all(&cpu_cache).expect("Failed to create CPU cache directory");

    if !cpu_cache.join("whisper.dll").exists() {
        if !cpu_zip.exists() {
            let url = format!(
                "{}/v{}/whisper-bin-x64.zip",
                GITHUB_RELEASE_BASE, WHISPER_VERSION
            );
            println!(
                "cargo:warning=Downloading CPU whisper.cpp binaries from: {}",
                url
            );
            download_file(&url, &cpu_zip).expect("Failed to download CPU whisper.cpp binary");
        }
        println!("cargo:warning=Extracting CPU whisper.cpp libraries...");
        extract_zip_libraries(&cpu_zip, &cpu_cache, &cpu_libs)
            .expect("Failed to extract CPU whisper.cpp libraries");
    }

    // Copy to target/{profile}/cuda/ and target/{profile}/cpu/ for runtime
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let runtime_dir = target_dir.join(&profile);
    let release_dir = target_dir.join("release");

    for dir in [&runtime_dir, &release_dir] {
        let cuda_dest = dir.join("cuda");
        let cpu_dest = dir.join("cpu");
        let _ = fs::create_dir_all(&cuda_dest);
        let _ = fs::create_dir_all(&cpu_dest);

        for lib in &cuda_libs {
            let src = cuda_cache.join(lib);
            let dest = cuda_dest.join(lib);
            if src.exists() {
                copy_if_changed(&src, &dest, lib);
            }
        }
        for lib in &cpu_libs {
            let src = cpu_cache.join(lib);
            let dest = cpu_dest.join(lib);
            if src.exists() {
                copy_if_changed(&src, &dest, lib);
            }
        }
    }

    println!("cargo:warning=Windows x64: bundled both CUDA and CPU whisper.cpp variants");
}

/// Copy a file only if the destination doesn't exist or has a different size.
fn copy_if_changed(src: &Path, dest: &Path, label: &str) {
    let needs_copy = if dest.exists() {
        fs::metadata(src).map(|m| m.len()).unwrap_or(0)
            != fs::metadata(dest).map(|m| m.len()).unwrap_or(0)
    } else {
        true
    };
    if needs_copy {
        if let Err(e) = fs::copy(src, dest) {
            println!("cargo:warning=Failed to copy {}: {}", label, e);
        }
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let response = reqwest::blocking::Client::builder()
        .user_agent("vtx-engine-build")
        .timeout(Duration::from_secs(300)) // 5 minute timeout for large files
        .build()?
        .get(url)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {} for URL: {}", response.status(), url).into());
    }

    let bytes = response.bytes()?;
    let mut file = fs::File::create(dest)?;
    file.write_all(&bytes)?;

    Ok(())
}

fn extract_zip_libraries(
    zip_path: &Path,
    output_dir: &Path,
    lib_names: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut found = vec![false; lib_names.len()];

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        for (idx, lib_name) in lib_names.iter().enumerate() {
            if !found[idx] && name.ends_with(lib_name) && !name.contains("__MACOSX") {
                let output_path = output_dir.join(lib_name);
                let mut output_file = fs::File::create(&output_path)?;
                io::copy(&mut file, &mut output_file)?;
                println!("cargo:warning=Extracted {}", lib_name);
                found[idx] = true;
                break;
            }
        }
    }

    // CUDA-only DLLs (ggml-cuda.dll, cublas*, cudart*) are non-fatal if absent
    // in a CPU-only zip. Only whisper.dll and ggml.dll are required.
    for (idx, lib_name) in lib_names.iter().enumerate() {
        if !found[idx] {
            let required = *lib_name == "whisper.dll" || *lib_name == "ggml.dll";
            if required {
                return Err(format!("Required library {} not found in archive", lib_name).into());
            } else {
                println!(
                    "cargo:warning={} not found in archive (non-fatal)",
                    lib_name
                );
            }
        }
    }

    Ok(())
}

fn build_whisper_linux(cuda_enabled: bool) {
    println!("cargo:warning=Building whisper.cpp from source for Linux");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let whisper_dir = out_dir.join("whisper.cpp");
    let build_dir = whisper_dir.join("build");

    // Clone or update whisper.cpp
    if !whisper_dir.exists() {
        let status = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                &format!("v{}", WHISPER_VERSION),
                "https://github.com/ggml-org/whisper.cpp.git",
                whisper_dir.to_str().unwrap(),
            ])
            .status()
            .expect("Failed to clone whisper.cpp");

        if !status.success() {
            panic!("Failed to clone whisper.cpp");
        }
    }

    fs::create_dir_all(&build_dir).expect("Failed to create build directory");

    let mut cmake_args = vec![
        "-DBUILD_SHARED_LIBS=ON".to_string(),
        "-DCMAKE_BUILD_TYPE=Release".to_string(),
    ];

    if cuda_enabled {
        cmake_args.push("-DGGML_CUDA=ON".to_string());
    }

    cmake_args.push("..".to_string());

    let status = std::process::Command::new("cmake")
        .args(&cmake_args)
        .current_dir(&build_dir)
        .status()
        .expect("Failed to run cmake");

    if !status.success() {
        panic!("cmake configuration failed");
    }

    let status = std::process::Command::new("cmake")
        .args(["--build", ".", "--config", "Release", "-j"])
        .current_dir(&build_dir)
        .status()
        .expect("Failed to build whisper.cpp");

    if !status.success() {
        panic!("whisper.cpp build failed");
    }

    // Copy built libraries to OUT_DIR
    for lib_name in &["libwhisper.so", "libggml.so"] {
        let search_dirs = [
            build_dir.join("src"),
            build_dir.join("ggml/src"),
            build_dir.clone(),
        ];

        for dir in &search_dirs {
            let src = dir.join(lib_name);
            if src.exists() {
                let dest = out_dir.join(lib_name);
                fs::copy(&src, &dest)
                    .unwrap_or_else(|e| panic!("Failed to copy {}: {}", lib_name, e));
                println!("cargo:warning=Copied {} from {}", lib_name, dir.display());
                break;
            }
        }
    }
}
