//! Basic 3D highway scene example
//!
//! Demonstrates the infinite highway UI concept with:
//! - Grid road surface
//! - Cyan waveform walls (static geometry for now)
//! - Flight camera controls
//! - Atmospheric lighting and fog
//!
//! Controls:
//!   W/A/S/D - Move forward/left/back/right
//!   Q/E - Move up/down
//!   Shift - Speed boost
//!   Right Mouse + Move - Look around
//!   Esc - Exit

use vvdaw_ui_3d::create_app;

fn main() {
    println!("Starting 3D Highway UI...");
    println!();
    println!("Controls:");
    println!("  W/A/S/D - Move");
    println!("  Q/E - Up/Down");
    println!("  Shift - Speed boost");
    println!("  Right Mouse + Move - Look around");
    println!("  Esc - Exit");
    println!();

    create_app().run();
}
