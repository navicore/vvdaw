# vvdaw - Visual Virtual DAW

An experimental DAW with a 2D/3D game-like interface built with Rust, Bevy, and VST3 plugin hosting.

## Project Structure

This is a cargo workspace containing multiple crates:

### Core Crates

- **vvdaw-core** - Shared types, traits, and constants
- **vvdaw-comms** - Lockless communication primitives (ring buffers, triple buffers)
- **vvdaw-plugin** - Plugin abstraction layer (format-agnostic trait)

### Audio Crates

- **vvdaw-audio** - Audio engine, processing graph, and cpal integration
- **vvdaw-vst3** - VST3 plugin host implementation
- **vvdaw-clap** - CLAP plugin host implementation (future)

### UI Crate

- **vvdaw-ui** - Bevy-based 2D/3D visualization and interaction

### Application

- **vvdaw-app** - Main binary that ties everything together

## Architecture Principles

1. **Audio Thread Isolation**: Audio processing runs on a dedicated real-time thread with no allocations or blocking operations
2. **Lockless Communication**: UI and audio threads communicate via ring buffers and atomic operations
3. **Plugin Format Agnostic**: Core audio engine works with an abstract Plugin trait, allowing multiple plugin formats
4. **ECS-based UI**: Bevy's Entity Component System for flexible, performant UI

## Building

### Prerequisites

Before building, you need to download the VST3 SDK (MIT licensed):

```bash
# Download and setup the VST3 SDK
./scripts/setup-vst3-sdk.sh
```

This will clone the official Steinberg VST3 SDK into `vendor/vst3sdk`.

### Build Commands

```bash
# Build all crates
cargo build

# Build release
cargo build --release

# Run the application
cargo run --bin vvdaw

# Run tests
cargo test
```

## Dependencies

- **Bevy 0.15** - Game engine for UI
- **cpal 0.15** - Cross-platform audio I/O
- **VST3 SDK** (MIT) - Rust bindings generated via bindgen
- **rtrb** - Real-time safe ring buffer
- **triple-buffer** - Lock-free triple buffering
- **bindgen** - Generate Rust FFI bindings from C/C++ headers
- **libloading** - Dynamic library loading for plugins

## Examples

See [`crates/vvdaw-app/examples/`](crates/vvdaw-app/examples/) for working examples:

- **load_vst3** - Complete workflow for loading and processing audio through a VST3 plugin

```bash
cargo run --example load_vst3 -- /path/to/plugin.vst3
```

## Development Status

🚧 **Early Development** - Currently in proof-of-concept phase

### Current Phase: VST3 Plugin Host

- [x] Workspace structure
- [x] Core type definitions
- [x] Communication primitives API
- [x] Audio engine implementation (cpal + real-time thread)
- [x] VST3 plugin loading (complete COM interface wrapping)
- [x] AudioGraph with plugin integration
- [x] Working VST3 plugin example
- [ ] Basic UI

## License

MIT OR Apache-2.0
