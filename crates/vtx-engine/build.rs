//! Build script for vtx-engine
//!
//! Downloads prebuilt whisper.cpp binaries for the target platform.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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

    let (zip_name, lib_names): (String, Vec<&str>) = match (
        target_os.as_str(),
        target_arch.as_str(),
    ) {
        ("windows", "x86_64") => {
            println!(
                "cargo:warning=Downloading whisper.cpp v{} for Windows x64 (CUDA-enabled with CPU fallback)",
                WHISPER_VERSION
            );
            (
                "whisper-cublas-12.4.0-bin-x64.zip".to_string(),
                vec![
                    "whisper.dll",
                    "ggml.dll",
                    "ggml-base.dll",
                    "ggml-cpu.dll",
                    "ggml-cuda.dll",
                    "cublas64_12.dll",
                    "cublasLt64_12.dll",
                    "cudart64_12.dll",
                ],
            )
        }
        ("macos", _) => {
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

            // Search for the file in the archive (may be in a subdirectory)
            let mut found = false;
            for i in 0..archive.len() {
                let mut file = archive.by_index(i).expect("Failed to read zip entry");
                let name = file.name().to_string();

                if name.ends_with(lib_name) && !name.contains("__MACOSX") {
                    let mut dest = fs::File::create(&dest_path)
                        .unwrap_or_else(|e| panic!("Failed to create {}: {}", lib_name, e));
                    io::copy(&mut file, &mut dest)
                        .unwrap_or_else(|e| panic!("Failed to extract {}: {}", lib_name, e));
                    found = true;
                    println!(
                        "cargo:warning=Extracted {} ({} bytes)",
                        lib_name,
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
