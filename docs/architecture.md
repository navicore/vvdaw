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
**Purpose**: Universal plugin abstraction trait
**Dependencies**: vvdaw-core

**The Plugin Trait - Core Abstraction**:

The `Plugin` trait is the **universal interface** for all audio processing in vvdaw. Everything that processes audio - built-in processors, VST3 plugins, CLAP plugins, test mocks - implements this trait.

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

**Why This Matters**:
- Graph code is **completely agnostic** to plugin implementation
- Same parameter automation for all plugin types
- Consistent session serialization
- Easy testing with mock implementations

#### Built-in Processors

**Purpose**: Core audio utilities implemented directly in Rust
**Location**: `vvdaw-audio/src/builtin/`
**Implementation**: Direct `Plugin` trait implementation

Built-in processors are **first-class plugins** that happen to be implemented in Rust rather than loaded from external formats. They have zero overhead compared to external plugins (no IPC, no FFI, direct function calls).

**Examples**:
- `GainProcessor` - Per-channel gain control
- `PanProcessor` - Stereo panning
- `MixerProcessor` - Multi-input mixing
- `PhaseInvertProcessor` - Phase inversion utility
- `MuteProcessor` - Mute/solo functionality

**Why Built-in Processors Don't Use VST3**:
- No subprocess overhead
- No shared memory IPC
- No parameter serialization across process boundaries
- Direct vtable dispatch only
- But same `Plugin` interface, so graph code doesn't care

#### vvdaw-vst3
**Purpose**: VST3 format host implementation
**Dependencies**: vvdaw-core, vvdaw-plugin, bindgen (build-time)
**Responsibilities**:
- Load VST3 plugins from .vst3 bundles
- Wrap VST3 COM interfaces to implement `Plugin` trait
- Handle VST3-specific lifecycle (IComponent, IProcessor, etc.)
- Manage VST3 GUI windows (native, not Bevy-rendered)
- Provide subprocess isolation via `MultiProcessPlugin`

**Key Implementation**:
```rust
// vvdaw-vst3 provides two implementations:
pub struct Vst3Plugin { /* ... */ }      // In-process VST3
pub struct MultiProcessPlugin { /* ... */ } // Subprocess-isolated VST3

// Both implement Plugin trait:
impl Plugin for Vst3Plugin { /* ... */ }
impl Plugin for MultiProcessPlugin { /* ... */ }
```

#### vvdaw-clap
**Purpose**: CLAP format host implementation (future)
**Dependencies**: vvdaw-core, vvdaw-plugin
**Status**: Future work - will implement `Plugin` trait for CLAP plugins
**Implementation Strategy**: Same as VST3 - wrap CLAP API to implement `Plugin`

### Audio Layer

#### vvdaw-audio
**Purpose**: Audio engine, processing graph, and built-in processors
**Dependencies**: vvdaw-core, vvdaw-plugin, vvdaw-comms, cpal, dasp, ron, serde
**Key Components**:

**AudioEngine**:
- Manages cpal audio stream
- Owns the audio thread
- Receives commands from UI thread via `AudioChannels`
- Processes audio callbacks

**AudioGraph**:
- Directed graph of audio nodes
- Each node wraps a `Box<dyn Plugin>` instance
- **Completely agnostic to plugin type** (built-in, VST3, CLAP, etc.)
- Topological sort for processing order
- Buffer management and routing
- Cycle detection with fallback to linear ordering

**Key Insight**: Graph only knows about the `Plugin` trait:
```rust
pub struct AudioNode {
    pub plugin: Box<dyn Plugin>,  // Could be anything!
    // ... routing info
}

// Graph doesn't care about implementation:
for node_id in &self.processing_order {
    let node = &mut self.nodes[node_id];
    node.plugin.process(&mut audio_buffer, &event_buffer)?;
}
```

**Session**:
- Serialize/deserialize audio graphs to RON format
- Stores plugin sources and parameter values
- Validates plugin paths on load (security)
- Reconstructs graphs with proper plugin instances

**PluginSource** - Tracks where plugins come from:
```rust
pub enum PluginSource {
    Builtin { name: String },           // "gain", "pan", etc.
    Vst3 { path: PathBuf },            // "/Library/.../Plugin.vst3"
    // Future: Clap { path: PathBuf },
}
```

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

### The Universal Node Model

**Critical Insight**: There is only **one node type** in vvdaw - `AudioNode` containing `Box<dyn Plugin>`.

The graph doesn't distinguish between:
- Built-in processors (Rust implementations)
- VST3 plugins (C++ wrapped via FFI)
- CLAP plugins (future)
- Test mocks

Everything implements the same `Plugin` trait, so the graph treats them identically.

### Common Node Patterns

While the graph sees all nodes the same way, common **logical** patterns include:

1. **Built-in Utility Nodes**: `GainProcessor`, `PanProcessor`, `MixerProcessor`
2. **External Effect Nodes**: VST3/CLAP reverbs, delays, compressors
3. **External Instrument Nodes**: VST3/CLAP synthesizers, samplers
4. **Future I/O Nodes**: Audio device input/output (also implements `Plugin`)

**Example**:
```rust
// All of these are just AudioNodes internally:
graph.add_node(Box::new(GainProcessor::default()), PluginSource::Builtin("gain"))?;
graph.add_node(Box::new(MultiProcessPlugin::spawn(path)?), PluginSource::Vst3(path))?;
graph.add_node(Box::new(MockPlugin::default()), PluginSource::Unknown)?;  // Testing
```

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

## Plugin Implementation Strategy

### The Plugin Trait as Abstraction Boundary

**One Interface, Multiple Implementations**:

```
                    ┌─────────────────┐
                    │  Plugin Trait   │
                    │  (Universal     │
                    │   Interface)    │
                    └────────┬────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
  ┌─────▼──────┐      ┌─────▼──────┐      ┌─────▼──────┐
  │  Built-in  │      │    VST3    │      │    CLAP    │
  │ Processors │      │  (wrapped) │      │  (future)  │
  └────────────┘      └────────────┘      └────────────┘
   - Direct Rust       - C FFI bridge      - C FFI bridge
   - Zero overhead     - IPC optional      - IPC optional
   - Real-time safe    - Subprocess        - Subprocess
```

### When to Use Each Implementation

**Built-in Processors** (`vvdaw-audio/src/builtin/`):
- ✅ Essential utilities (gain, pan, mixer, mute)
- ✅ Real-time critical operations
- ✅ Need direct access to host features
- ✅ Simplicity and reliability > flexibility
- ❌ Complex effects (reverb, synthesis) - use plugins

**VST3 Plugins** (`vvdaw-vst3`):
- ✅ Third-party effects and instruments
- ✅ Complex DSP algorithms
- ✅ Industry-standard compatibility
- ✅ Ecosystem of existing plugins

**CLAP Plugins** (future):
- ✅ Modern plugin API with better design than VST3
- ✅ More flexible parameter/modulation system
- ✅ Open-source friendly

### Performance Comparison

| Implementation | Function Call | IPC | FFI | Subprocess Isolation |
|---------------|---------------|-----|-----|---------------------|
| Built-in      | Direct vtable | No  | No  | No                  |
| VST3 (in-proc)| Via FFI       | No  | Yes | No                  |
| VST3 (multi)  | Via FFI       | Yes | Yes | Yes                 |
| CLAP (future) | Via FFI       | TBD | Yes | TBD                 |

**Recommendation**: Keep built-ins minimal and essential. Let the plugin ecosystem handle everything else.

## Session Format and Serialization

### Session Structure (RON)

```ron
(
    version: 1,
    name: "My Project",
    sample_rate: 48000,
    block_size: 512,
    graph: (
        nodes: [
            (
                id: 0,
                plugin: Builtin(name: "gain"),
                parameters: {0: 0.75},  // 75% gain
                inputs: 2,
                outputs: 2,
            ),
            (
                id: 1,
                plugin: Vst3(
                    path: "/Library/Audio/Plug-Ins/VST3/MyReverb.vst3",
                    parameters: {5: 0.5, 10: 0.8},
                ),
                inputs: 2,
                outputs: 2,
            ),
        ],
        connections: [
            (from: 0, to: 1),
        ],
    ),
)
```

### Session Loading Strategy

```rust
// Session reconstruction with plugin loader callback
let graph = session.to_graph(|plugin_spec| {
    match plugin_spec {
        PluginSpec::Builtin { name, .. } => {
            // Fast: Just instantiate Rust struct
            builtin::create_builtin(name)
                .ok_or_else(|| format!("Unknown built-in: {}", name))
        }
        PluginSpec::Vst3 { path, .. } => {
            // Slower: Spawn subprocess, load VST3
            let plugin = MultiProcessPlugin::spawn(path)?;
            Ok(Box::new(plugin) as Box<dyn Plugin>)
        }
    }
})?;

// Parameters are restored after plugin instantiation
// Graph topology is rebuilt
// Result: Exact recreation of saved session
```

### Security Considerations

- **Path Validation**: Session loader validates all plugin paths
  - Must be absolute (no relative paths)
  - No directory traversal (`..` or `.`)
  - Warns on unexpected extensions
- **Unknown Sources**: Plugins added via audio thread have `PluginSource::Unknown` and can't be saved
- **Version Checking**: Session format has version field for future compatibility

## Proof of Concept Roadmap

### Phase 1: Foundation ✅
- [x] Workspace structure
- [x] Core types
- [x] Communication primitives
- [x] Build system
- [x] Plugin trait abstraction

### Phase 2: Audio Engine ✅
- [x] Implement cpal integration
- [x] Create audio thread with lock-free communication
- [x] Test command/event flow
- [x] AudioGraph with topological processing

### Phase 3: VST3 Plugin Host ✅
- [x] Load VST3 plugins with full SDK integration
- [x] Route audio through plugins
- [x] Parameter control (get/set)
- [x] Edit controller support
- [x] Multi-process isolation for plugin crashes
- [x] Subprocess-based plugin hosting

### Phase 4: Session Format ✅
- [x] RON-based serialization
- [x] Save/load audio graphs with plugin configurations
- [x] Parameter restoration
- [x] Path validation and security
- [x] CLI tool (`vvdaw-process`) for offline processing

### Phase 5: Built-in Processors (Next)
- [ ] Gain processor
- [ ] Pan processor
- [ ] Mixer processor
- [ ] Basic utility processors (mute, phase)
- [ ] Update PluginSource to include Builtin variant
- [ ] Integrate with session format

### Phase 6: Basic UI
- [ ] Bevy window with 2D view
- [ ] Visualize audio levels
- [ ] Add/remove plugin nodes
- [ ] Parameter sliders
- [ ] Integrate with audio engine via channels

### Phase 7: 2D Graph View
- [ ] Draw nodes as boxes
- [ ] Draw connections as lines
- [ ] Drag to connect
- [ ] Click to add plugins
- [ ] Save/load sessions from UI

### Phase 8: 3D Experimentation
- [ ] 3D camera controls
- [ ] Spatial node placement
- [ ] Visual feedback experiments

### Phase 9: CLAP Support (Future)
- [ ] CLAP plugin loading
- [ ] CLAP Plugin trait implementation
- [ ] Session format support for CLAP
- [ ] Multi-process isolation for CLAP

## Open Questions

1. **Channel Integration with Bevy**: How do we store `rtrb` channels given they're !Send + !Sync?
   - Option A: Thread-local storage
   - Option B: crossbeam wrapper for Bevy side
   - Option C: Custom integration outside ECS
   - Status: Can defer until UI integration phase

2. **Built-in Processor Scope**: Which processors should be built-in vs plugins?
   - Current thinking: Keep built-ins minimal (gain, pan, mixer, utility)
   - Complex effects should be VST3/CLAP plugins
   - Never port offline processing code (CDP) to real-time
   - Use CDP as algorithm reference only

3. **3D Metaphor**: What spatial representation makes sense for audio?
   - Patch cables in 3D space?
   - Timeline as Z-axis?
   - Frequency/amplitude visualization?
   - Defer until after 2D UI is solid

4. **Performance**: Can Bevy handle 60fps rendering + audio visualization?
   - Likely yes, but needs profiling
   - May need to throttle certain visualizations

5. **CLAP Integration**: When and how to add CLAP support?
   - Same pattern as VST3 (implement Plugin trait)
   - Likely easier API than VST3
   - Wait until after basic UI is working

## Key Architectural Principles

These principles guide all design decisions in vvdaw:

### 1. **Single Abstraction for Audio Processing**
- The `Plugin` trait is the **only** interface for audio processing
- Built-ins, VST3, CLAP, and test mocks all implement the same trait
- The graph is completely agnostic to implementation details
- **Benefit**: Consistency, testability, future plugin format support

### 2. **Real-Time Safety is Non-Negotiable**
- Audio callback: no allocations, no blocking, no I/O
- Lock-free communication between threads
- Pre-allocated buffers only
- **Benefit**: Professional-grade reliability and performance

### 3. **Plugin Format is an Implementation Detail**
- VST3 and CLAP are wrappers that implement `Plugin`
- The core engine doesn't know about VST3/CLAP specifics
- New formats can be added without touching graph code
- **Benefit**: Future-proof, maintainable, clean separation

### 4. **Built-ins are Minimal and Essential**
- Only utilities that benefit from being in-process
- Complex effects belong in plugins
- Never compromise real-time safety for features
- **Benefit**: Small, auditable codebase; leverage plugin ecosystem

### 5. **Offline ≠ Real-Time**
- Code designed for offline processing (like CDP) stays offline
- Use as algorithmic reference, not direct integration
- `vvdaw-process` is the home for offline operations
- **Benefit**: Keep architectural boundaries clear

### 6. **Crash Isolation via Multiprocess**
- Third-party plugins run in subprocesses
- Plugin crashes don't kill the DAW
- IPC overhead is acceptable for reliability
- **Benefit**: Professional-grade stability

### 7. **Type Safety and Rust Guarantees**
- Leverage Rust's type system for correctness
- Audio thread is `Send` but not `Sync` (single-threaded)
- Channels are `!Send + !Sync` (used correctly)
- **Benefit**: Catch bugs at compile time

## References

- VST3 SDK: https://github.com/steinbergmedia/vst3sdk
- CLAP: https://github.com/free-audio/clap
- cpal: https://github.com/RustAudio/cpal
- Bevy: https://bevyengine.org
- rtrb: https://github.com/mgeier/rtrb
- RON: https://github.com/ron-rs/ron
- Composer's Desktop Project (algorithm reference): https://github.com/ComposersDesktop/CDP8
- Prior art: AudioMulch, Usine Hollyhock, SunVox, Pd/Max
