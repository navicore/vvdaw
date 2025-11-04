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
- **vst3-sys/vst3-com** - VST3 plugin hosting
- **rtrb** - Real-time safe ring buffer
- **triple-buffer** - Lock-free triple buffering

## Development Status

🚧 **Early Development** - Currently in proof-of-concept phase

### Current Phase: Foundation

- [x] Workspace structure
- [x] Core type definitions
- [x] Communication primitives API
- [ ] Audio engine implementation
- [ ] VST3 plugin loading
- [ ] Basic UI

## License

MIT OR Apache-2.0
