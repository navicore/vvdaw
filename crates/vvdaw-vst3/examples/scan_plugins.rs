//! Example: Scan for installed VST3 plugins
//!
//! This example discovers all VST3 plugins installed on the system
//! and displays their information.
//!
//! Run with: `cargo run --example scan_plugins --release`

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    println!("VST3 Plugin Scanner");
    println!("===================\n");
    println!("Scanning system VST3 directories...\n");
    let plugins = vvdaw_vst3::Vst3Loader::scan_system();
    if plugins.is_empty() {
        println!("No VST3 plugins found.");
        println!("\nStandard VST3 locations:");
        #[cfg(target_os = "macos")]
        {
            println!("  - /Library/Audio/Plug-Ins/VST3");
            println!("  - ~/Library/Audio/Plug-Ins/VST3");
        }
        #[cfg(target_os = "windows")]
        {
            println!("  - C:\\Program Files\\Common Files\\VST3");
            println!("  - %LOCALAPPDATA%\\Programs\\Common\\VST3");
        }
        #[cfg(target_os = "linux")]
        {
            println!("  - ~/.vst3");
            println!("  - /usr/lib/vst3");
            println!("  - /usr/local/lib/vst3");
        }
    } else {
        println!("Found {} VST3 plugin(s):\n", plugins.len());

        // Group plugins by vendor
        let mut by_vendor: std::collections::BTreeMap<String, Vec<&vvdaw_plugin::PluginInfo>> =
            std::collections::BTreeMap::new();

        for plugin in &plugins {
            by_vendor
                .entry(plugin.vendor.clone())
                .or_default()
                .push(plugin);
        }

        for (vendor, vendor_plugins) in by_vendor {
            println!("{vendor}:");
            for plugin in vendor_plugins {
                println!("  - {} (v{})", plugin.name, plugin.version);
                println!("    ID: {}", plugin.unique_id);
            }
            println!();
        }

        println!("Total: {} plugins", plugins.len());
    }
}
