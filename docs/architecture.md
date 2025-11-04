# vvdaw Architecture Design

## Overview

vvdaw is a DAW with a 2D/3D game-like interface for audio production, built with Rust, Bevy, and VST3 plugin hosting.

## Core Design Principles

1. **Real-Time Audio Safety**: Audio processing happens on a dedicated thread with no allocations, no blocking, and deterministic latency
2. **Lockless Communication**: UI and audio threads communicate via lock-free data structures
3. **Plugin Format Agnostic**: Core audio engine works with abstract plugin interfaces
4. **ECS-based UI**: Bevy's Entity Component System for flexible, performant UI

## Crate Structure

### Foundation Layer

#### vvdaw-core
**Purpose**: Shared types and constants
**Dependencies**: None
**Key Types**:
- `Sample` = f32
- `SampleRate` = u32
- `Frames` = usize
- `ChannelCount` = usize
- Common error types

#### vvdaw-comms
**Purpose**: Lockless communication primitives
**Dependencies**: vvdaw-core, rtrb, triple_buffer, crossbeam-channel
**Key Types**:
- `AudioCommand` - Commands from UI → Audio (Start, Stop, SetParameter, etc.)
- `AudioEvent` - Events from Audio → UI (Started, Stopped, Error, PeakLevel)
- `UiChannels` - Channels for UI thread (sends commands, receives events)
- `AudioChannels` - Channels for audio thread (receives commands, sends events)

**Communication Flow**:
```
UI Thread                    Audio Thread
   |                              |
   |--- AudioCommand::Start ----->|
   |                              |
   |<--- AudioEvent::Started -----|
   |                              |
   |--- SetParameter(id, val) --->|
   |                              |
   |<--- PeakLevel{ch, level} ----|
```

### Plugin Layer

#### vvdaw-plugin
**Purpose**: Plugin abstraction trait
**Dependencies**: vvdaw-core
**Key Traits**:
```rust
pub trait Plugin: Send {
    fn info(&self) -> &PluginInfo;
    fn initialize(&mut self, sample_rate: SampleRate, max_block_size: Frames);
    fn process(&mut self, audio: &mut AudioBuffer, events: &EventBuffer);
    fn set_parameter(&mut self, id: u32, value: f32);
    fn get_parameter(&self, id: u32) -> f32;
    fn parameters(&self) -> Vec<ParameterInfo>;
    fn input_channels(&self) -> ChannelCount;
    fn output_channels(&self) -> ChannelCount;
    fn deactivate(&mut self);
}
```

**Key Types**:
- `AudioBuffer` - Interleaved input/output buffers
- `EventBuffer` - MIDI/parameter events with sample offsets
- `PluginInfo` - Metadata (name, vendor, version, unique_id)

#### vvdaw-vst3
**Purpose**: VST3 host implementation
**Dependencies**: vvdaw-core, vvdaw-plugin, vst3-sys (future)
**Responsibilities**:
- Load VST3 plugins from .vst3 bundles
- Wrap VST3 COM interfaces to implement `Plugin` trait
- Handle VST3-specific lifecycle (IComponent, IProcessor, etc.)
- Manage VST3 GUI windows (native, not Bevy-rendered)

#### vvdaw-clap
**Purpose**: CLAP host implementation (future)
**Dependencies**: vvdaw-core, vvdaw-plugin
**Status**: Stub - to be implemented after VST3

### Audio Layer

#### vvdaw-audio
**Purpose**: Audio engine and processing graph
**Dependencies**: vvdaw-core, vvdaw-plugin, vvdaw-comms, cpal, dasp
**Key Components**:

**AudioEngine**:
- Manages cpal audio stream
- Owns the audio thread
- Receives commands from UI thread via `AudioChannels`
- Processes audio callbacks

**AudioGraph**:
- Directed graph of audio nodes
- Each node wraps a `Plugin` instance
- Topological sort for processing order
- Buffer management and routing

**AudioConfig**:
```rust
pub struct AudioConfig {
    pub sample_rate: SampleRate,     // e.g., 48000
    pub block_size: Frames,          // e.g., 256
    pub input_channels: usize,       // e.g., 2
    pub output_channels: usize,      // e.g., 2
}
```

### UI Layer

#### vvdaw-ui
**Purpose**: Bevy-based 2D/3D UI
**Dependencies**: vvdaw-core, vvdaw-comms, bevy
**Key Components**:
- `VvdawUiPlugin` - Bevy plugin
- 2D/3D rendering systems
- Input handling (mouse, keyboard, MIDI controllers)
- Parameter automation visualization

**Challenges**:
- `rtrb` channels are !Send + !Sync, so they can't be Bevy resources
- Solutions to explore:
  - Thread-local storage
  - crossbeam-channel wrapper for Bevy side
  - Custom integration outside Bevy's ECS

### Application Layer

#### vvdaw-app
**Purpose**: Main binary that ties everything together
**Dependencies**: All other crates
**Responsibilities**:
- Initialize tracing/logging
- Create communication channels
- Start audio engine
- Start Bevy UI
- Coordinate shutdown

## Threading Model

```
┌─────────────────────────────────────────────────────────────┐
│                        Main Thread                          │
│  - Initialize app                                           │
│  - Create channels                                          │
│  - Spawn audio thread                                       │
│  - Run Bevy (blocks on main thread)                         │
└─────────────────────────────────────────────────────────────┘
                      │                  │
                      │                  │
         ┌────────────┴──────┐    ┌─────┴──────────┐
         │                   │    │                │
    ┌────▼─────────────┐     │    │   ┌────────────▼──────┐
    │  Audio Thread    │     │    │   │   Bevy Render     │
    │  (real-time)     │◄────┼────┼───│   Thread          │
    │                  │     │    │   │                   │
    │  - Process audio │     │    │   │   - Draw UI       │
    │  - Read commands │     │    │   │   - Handle input  │
    │  - Send events   │     │    │   │   - Send commands │
    │  - Call plugins  │     │    │   │   - Read events   │
    └──────────────────┘     │    │   └───────────────────┘
                             │    │
                        rtrb rings
```

## Audio Thread Constraints

**MUST**:
- Pre-allocate all buffers during initialization
- Use only wait-free/lock-free operations
- Complete processing within block deadline (e.g., ~5ms for 256 samples @ 48kHz)
- Be deterministic

**MUST NOT**:
- Allocate memory (no Vec::push, String, Box::new, etc.)
- Block on mutexes, channels, I/O
- Call system functions (file I/O, network, etc.)
- Use unbounded recursion

**Allowed Operations**:
- Read from `rtrb::Consumer` (lock-free, wait-free)
- Write to `rtrb::Producer` (lock-free, wait-free)
- Read/write atomics
- Call plugin `process()` methods
- Pure computation

## Audio Graph Design

### Node Types

1. **Plugin Nodes**: Wrap VST3/CLAP plugins
2. **Mixer Nodes**: Mix multiple inputs
3. **Send/Return Nodes**: Aux sends and returns
4. **I/O Nodes**: Audio device input/output

### Graph Topology

```
Input Device Node
       |
    ┌──▼───────┐
    │ Plugin 1 │
    └──┬───────┘
       │
    ┌──▼───────┐        ┌────────────┐
    │ Plugin 2 │────────► Send Node  │
    └──┬───────┘        └──┬─────────┘
       │                   │
       │                ┌──▼──────────┐
       │                │ Reverb      │
       │                │ Plugin      │
       │                └──┬──────────┘
       │                   │
       │                ┌──▼──────────┐
       └────────────────► Mixer Node  │
                        └──┬──────────┘
                           │
                    Output Device Node
```

### Processing Order

- Topological sort determines processing order
- Detect and break cycles
- Handle feedback loops with delay buffers

## VST3 Integration

### Loading Process

1. Scan .vst3 bundle (platform-specific path)
2. Load dynamic library
3. Query VST3 factory
4. Create IComponent and IProcessor
5. Initialize with audio config
6. Activate processing
7. Wrap in `Plugin` trait

### Parameter Handling

- VST3 uses normalized parameters [0.0, 1.0]
- Our `Plugin` trait exposes raw min/max ranges
- Conversion happens in the wrapper

### GUI Integration (Future)

- VST3 plugins create native OS windows
- We'll need to integrate with Bevy's windowing
- Options:
  - Embed native windows
  - Parent VST3 window to Bevy window
  - Use VST3's IPlugView interface

## Proof of Concept Roadmap

### Phase 1: Foundation (Current)
- [x] Workspace structure
- [x] Core types
- [x] Communication primitives
- [x] Build system

### Phase 2: Audio Engine
- [ ] Implement cpal integration
- [ ] Create audio thread with proper priorities
- [ ] Test command/event flow
- [ ] Measure latency

### Phase 3: Simple Plugin Host
- [ ] Load a simple VST3 plugin (e.g., gain plugin)
- [ ] Route audio through plugin
- [ ] Control parameters from code

### Phase 4: Basic UI
- [ ] Bevy window with 2D view
- [ ] Visualize audio levels
- [ ] Add/remove plugin nodes
- [ ] Parameter sliders

### Phase 5: 2D Graph View
- [ ] Draw nodes as boxes
- [ ] Draw connections as lines
- [ ] Drag to connect
- [ ] Click to add plugins

### Phase 6: 3D Experimentation
- [ ] 3D camera controls
- [ ] Spatial node placement
- [ ] Visual feedback experiments

## Open Questions

1. **Channel Integration with Bevy**: How do we store `rtrb` channels given they're !Send + !Sync?
   - Option A: Thread-local storage
   - Option B: crossbeam wrapper for Bevy
   - Option C: Custom integration outside ECS

2. **VST3 Rust Bindings**: The crates.io vst3-sys may still be GPL
   - Option A: Wait for MIT-licensed version
   - Option B: Generate our own bindings from Steinberg's MIT SDK
   - Option C: Use wrapper crate like vst3-rs

3. **3D Metaphor**: What spatial representation makes sense for audio?
   - Patch cables in 3D space?
   - Timeline as Z-axis?
   - Frequency/amplitude visualization?

4. **Performance**: Can Bevy handle 60fps rendering + audio visualization?
   - Likely yes, but needs profiling
   - May need to throttle certain visualizations

## References

- VST3 SDK: https://github.com/steinbergmedia/vst3sdk
- cpal: https://github.com/RustAudio/cpal
- Bevy: https://bevyengine.org
- rtrb: https://github.com/mgeier/rtrb
- Prior art: AudioMulch, Usine Hollyhock, SunVox, Pd/Max
