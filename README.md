# vvdaw - Visual Virtual DAW

> ⚠️ **PRE-ALPHA SOFTWARE** - Under active development. Not ready for production use.
> APIs are unstable and will change without notice.

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

### UI Crates

- **vvdaw-ui** - Bevy-based 2D traditional UI with file browser and playback controls
- **vvdaw-ui-3d** - Experimental 3D "highway" visualization with scrolling waveform walls

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

# Run the application (defaults to 3D mode)
cargo run --bin vvdaw

# Run with 2D UI
cargo run --bin vvdaw -- --ui 2d

# Run with 3D UI (default)
cargo run --bin vvdaw -- --ui 3d

# Run tests
cargo test
```

## Dependencies

- **Bevy 0.17** - Game engine for UI
- **cpal 0.15** - Cross-platform audio I/O
- **VST3 SDK** (MIT) - Rust bindings generated via bindgen
- **rtrb 0.3** - Real-time safe ring buffer
- **triple-buffer 8.0** - Lock-free triple buffering
- **dasp 0.11** - Digital audio signal processing (resampling)
- **hound 3.5** - WAV file I/O
- **bindgen 0.72** - Generate Rust FFI bindings from C/C++ headers
- **libloading 0.8** - Dynamic library loading for plugins

## Examples

See [`crates/vvdaw-app/examples/`](crates/vvdaw-app/examples/) for working examples:

- **load_vst3** - Complete workflow for loading and processing audio through a VST3 plugin

```bash
cargo run --example load_vst3 -- /path/to/plugin.vst3
```

## Development Status

🚧 **Pre-Alpha** - Core functionality working but under heavy development

### Completed Features

- [x] Workspace structure with 8 crates
- [x] Core type definitions and plugin abstraction
- [x] Lock-free communication primitives (rtrb ring buffers)
- [x] Real-time audio engine (cpal + dedicated audio thread)
- [x] VST3 plugin loading and hosting
- [x] Audio processing graph with node management
- [x] 2D UI with file browser and playback controls
- [x] **3D "Highway" UI** - Experimental visualization with:
  - Scrolling waveform walls following playback
  - Real-time audio playback with position tracking
  - Load-time sample rate conversion (44.1kHz ↔ 48kHz)
  - Window-based waveform rendering for constant memory
  - Free-flying camera controls

### In Progress / Planned

- [ ] Timeline/sequencer view
- [ ] MIDI support
- [ ] Mixer with effects routing
- [ ] Additional plugin formats (CLAP, LV2)
- [ ] Automation recording and playback
- [ ] Project save/load

## License

MIT
