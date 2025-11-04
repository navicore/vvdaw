//! CLAP plugin host implementation.
//!
//! This crate will implement CLAP plugin hosting functionality.
//! Currently a stub - to be implemented after VST3 is working.

use vvdaw_plugin::PluginInfo;

/// CLAP plugin wrapper (stub)
#[allow(dead_code)]
pub struct ClapPlugin {
    info: PluginInfo,
}

// TODO: Implement Plugin trait for ClapPlugin when ready

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Placeholder test - to be replaced when CLAP implementation begins
    }
}
