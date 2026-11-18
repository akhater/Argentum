use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[path = "src/mods/build_ort.rs"]
mod build_ort; // Argentum: no re-hash, no download of a library already on this disk
#[path = "src/mods/mirror.rs"]
mod mirror; // Argentum: where our copies of downloaded files are kept

fn verify_sha256(path: &Path, expected_hash: &str) -> Result<bool, io::Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash_bytes = hasher.finalize();
    let calculated_hash = hex::encode(hash_bytes);
    Ok(calculated_hash == expected_hash)
}

fn download_and_verify(
    url: &str,
    dest_path: &Path,
    expected_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let temp_filename = dest_path.file_name().unwrap();
    let temp_path = out_dir.join(temp_filename);

    println!(
        "cargo:warning=Downloading to temporary path: {:?}",
        temp_path
    );
    let mut response = reqwest::blocking::get(url)?;

    if !response.status().is_success() {
        let status = response.status();
        let error_body = response
            .text()
            .unwrap_or_else(|_| "Could not read error body".to_string());
        return Err(format!("Download failed with status {}: {}", status, error_body).into());
    }

    let mut temp_file = fs::File::create(&temp_path)?;
    response.copy_to(&mut temp_file)?;
    println!("cargo:warning=Download complete. Verifying file integrity...");

    match verify_sha256(&temp_path, expected_hash) {
        Ok(true) => {
            fs::copy(&temp_path, dest_path)?;
            fs::remove_file(&temp_path)?;
            println!(
                "cargo:warning=Successfully downloaded and verified {:?}.",
                dest_path
            );
            Ok(())
        }
        Ok(false) => {
            fs::remove_file(&temp_path)?;
            Err("Verification failed! The downloaded file is corrupt.".into())
        }
        Err(e) => {
            fs::remove_file(&temp_path).ok();
            Err(format!("Could not verify file after download: {}", e).into())
        }
    }
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let runtime = build_ort::RUNTIMES
        .iter()
        .find(|r| r.os == target_os && r.arch == target_arch)
        .unwrap_or_else(|| panic!("Unsupported target: {}-{}", target_os, target_arch));
    let (lib_name, expected_hash) = (runtime.lib, runtime.sha256);

    let dest_dir = manifest_dir.join("resources");

    fs::create_dir_all(&dest_dir).unwrap();
    let dest_path = dest_dir.join(lib_name);

    let mut is_valid = build_ort::present(&dest_path, expected_hash);
    if !is_valid && dest_path.exists() {
        match verify_sha256(&dest_path, expected_hash) {
            Ok(true) => {
                println!(
                    "cargo:warning=ONNX Runtime library already exists and is valid. Skipping download."
                );
                is_valid = true;
            }
            Ok(false) => {
                println!(
                    "cargo:warning=File {:?} exists but has incorrect hash. Deleting and re-downloading.",
                    dest_path
                );
                fs::remove_file(&dest_path).unwrap();
            }
            Err(e) => {
                println!(
                    "cargo:warning=Could not verify file {:?}: {}. Re-downloading.",
                    dest_path, e
                );
            }
        }
    }

    if !is_valid {
        println!(
            "cargo:warning=Downloading ONNX Runtime library for {}-{}...",
            target_os, target_arch
        );
        // Argentum: our copy first, then RapidRAW's (`build_ort::Runtime::urls`).
        let mut failures = Vec::new();
        let fetched = runtime.urls().iter().any(|download_url| {
            println!("cargo:warning=URL: {}", download_url);
            download_and_verify(download_url, &dest_path, expected_hash)
                .map_err(|e| failures.push(format!("{download_url}: {e}")))
                .is_ok()
        });
        if !fetched {
            panic!(
                "Failed to download and verify ONNX Runtime library: {}",
                failures.join("; ")
            );
        }
    }

    println!("cargo:rerun-if-changed=build.rs");

    tauri_build::build()
}
