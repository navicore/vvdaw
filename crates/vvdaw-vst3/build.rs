//! Build script to generate VST3 bindings from the MIT-licensed VST3 SDK
//!
//! This script uses bindgen to generate Rust FFI bindings from the VST3 C++ headers.
//!
//! ## VST3 SDK Location
//!
//! The script looks for the VST3 SDK in the following locations (in order):
//! 1. `VST3_SDK_DIR` environment variable
//! 2. `vendor/vst3sdk` relative to workspace root
//! 3. `~/.vst3sdk` in the user's home directory
//!
//! ## Usage
//!
//! To use a custom SDK location, set the environment variable:
//! ```sh
//! export VST3_SDK_DIR=/path/to/vst3sdk
//! cargo build
//! ```
//!
//! Or clone the SDK into the vendor directory:
//! ```sh
//! git clone https://github.com/steinbergmedia/vst3sdk.git vendor/vst3sdk
//! cargo build
//! ```

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=VST3_SDK_DIR");

    // Find the VST3 SDK
    let sdk_path = find_vst3_sdk().expect(
        "VST3 SDK not found. Please either:
        1. Set VST3_SDK_DIR environment variable to point to your VST3 SDK
        2. Clone the SDK to vendor/vst3sdk (relative to workspace root)
        3. Clone the SDK to ~/.vst3sdk

        Example:
            git clone https://github.com/steinbergmedia/vst3sdk.git vendor/vst3sdk
        ",
    );

    println!("cargo:warning=Using VST3 SDK from: {}", sdk_path.display());
    println!("cargo:rerun-if-changed={}", sdk_path.display());

    // Generate bindings
    generate_bindings(&sdk_path);
}

/// Find the VST3 SDK directory
fn find_vst3_sdk() -> Option<PathBuf> {
    // 1. Check environment variable
    if let Ok(sdk_dir) = env::var("VST3_SDK_DIR") {
        let path = PathBuf::from(sdk_dir);
        if path.exists() {
            return Some(path);
        }
    }

    // 2. Check vendor directory (relative to workspace root)
    let workspace_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    let vendor_path = workspace_root.join("vendor").join("vst3sdk");
    if vendor_path.exists() {
        return Some(vendor_path);
    }

    // 3. Check home directory
    if let Ok(home) = env::var("HOME") {
        let home_path = PathBuf::from(home).join(".vst3sdk");
        if home_path.exists() {
            return Some(home_path);
        }
    }

    None
}

/// Generate Rust bindings from VST3 C++ headers
fn generate_bindings(sdk_path: &Path) {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Verify critical directories exist
    let pluginterfaces = sdk_path.join("pluginterfaces");
    assert!(
        pluginterfaces.exists(),
        "VST3 SDK pluginterfaces directory not found at: {}",
        pluginterfaces.display()
    );

    // We'll generate bindings for the core VST3 interfaces
    // Start with a minimal set and expand as needed
    let wrapper_header = out_path.join("vst3_wrapper.h");

    // Create a wrapper header that includes the VST3 interfaces we need
    std::fs::write(
        &wrapper_header,
        r#"
// Core VST3 interfaces

// Base types
#include "pluginterfaces/base/ftypes.h"
#include "pluginterfaces/base/fplatform.h"
#include "pluginterfaces/base/funknown.h"
#include "pluginterfaces/base/ipluginbase.h"

// VST3 component interfaces
#include "pluginterfaces/vst/ivstaudioprocessor.h"
#include "pluginterfaces/vst/ivstcomponent.h"
#include "pluginterfaces/vst/ivsteditcontroller.h"
#include "pluginterfaces/vst/ivstparameterchanges.h"
#include "pluginterfaces/vst/ivstprocesscontext.h"
#include "pluginterfaces/vst/vsttypes.h"

// Plugin factory
#include "pluginterfaces/vst/ivstpluginterfacesupport.h"
"#,
    )
    .expect("Failed to create wrapper header");

    println!("cargo:warning=Generating VST3 bindings from wrapper header");

    // Generate bindings using bindgen
    let bindings = bindgen::Builder::default()
        .header(wrapper_header.to_str().unwrap())
        // Include paths for VST3 SDK - include from SDK root
        .clang_arg(format!("-I{}", sdk_path.display()))
        // Platform-specific settings
        .clang_arg("-xc++")
        .clang_arg("-std=c++14")
        // Rust 2024 edition requires unsafe extern blocks
        .wrap_unsafe_ops(true)
        // Generate bindings for VST3 types
        .allowlist_type("Steinberg::.*")
        .allowlist_function("Steinberg::.*")
        .allowlist_var("Steinberg::.*")
        // Opaque types for complex C++ structures we'll handle manually
        .opaque_type("std::.*")
        // Disable documentation generation (C++ comments don't work as Rust doctests)
        .generate_comments(false)
        // Derive defaults where possible
        .derive_default(true)
        .derive_debug(true)
        .derive_copy(true)
        // Use core instead of std for no_std compatibility later
        .use_core()
        // Disable layout tests (they generate doc tests that fail on C++ comments)
        .layout_tests(false)
        // Prepend regular comments (not inner doc comments since this goes in a module)
        .raw_line("// Auto-generated VST3 FFI bindings from the MIT-licensed VST3 SDK")
        // Generate the bindings
        .generate()
        .expect("Failed to generate VST3 bindings");

    // Write bindings to OUT_DIR
    let bindings_path = out_path.join("vst3_bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Failed to write VST3 bindings");

    println!(
        "cargo:warning=VST3 bindings generated at: {}",
        bindings_path.display()
    );
}
