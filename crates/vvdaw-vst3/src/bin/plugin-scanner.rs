//! Standalone VST3 Plugin Scanner
//!
//! This binary runs in a separate process to scan a single VST3 plugin.
//! It loads the plugin, extracts metadata, and outputs JSON to stdout.
//!
//! This isolates the scanning process so that:
//! - Plugin crashes don't kill the main application
//! - Each plugin gets its own Objective-C runtime (no class conflicts)
//! - Multiple plugins can be scanned in parallel
//!
//! Usage: plugin-scanner <path-to-plugin.vst3>
//! Output: JSON-encoded `PluginInfo` or error message

use std::env;
use std::process;

#[derive(serde::Serialize)]
#[serde(tag = "status")]
enum ScanResult {
    #[serde(rename = "success")]
    Success { plugin: vvdaw_plugin::PluginInfo },

    #[serde(rename = "error")]
    Error { message: String },
}

fn main() {
    // Get plugin path from command line argument
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        let error = ScanResult::Error {
            message: "Usage: plugin-scanner <path-to-plugin.vst3>".to_string(),
        };
        println!("{}", serde_json::to_string(&error).unwrap());
        process::exit(1);
    }

    let plugin_path = &args[1];

    // Try to load plugin and extract info
    match load_plugin_info(plugin_path) {
        Ok(info) => {
            let result = ScanResult::Success { plugin: info };
            println!("{}", serde_json::to_string(&result).unwrap());
            process::exit(0);
        }
        Err(e) => {
            let result = ScanResult::Error {
                message: format!("Failed to load plugin: {e}"),
            };
            println!("{}", serde_json::to_string(&result).unwrap());
            process::exit(1);
        }
    }
}

/// Load plugin info using the existing loader infrastructure
fn load_plugin_info(path: &str) -> Result<vvdaw_plugin::PluginInfo, vvdaw_plugin::PluginError> {
    // Use the existing load_plugin_info_internal method
    // This loads the library, queries factory, extracts info, then unloads
    vvdaw_vst3::Vst3Loader::load_plugin_info_internal(path)
}
