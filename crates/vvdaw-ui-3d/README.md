# vvdaw-ui-3d

Experimental 3D UI for vvdaw - "Infinite Highway" interface.

## Concept

An immersive 3D interface where audio tracks are represented as infinite highways:
- **Road surface** = Timeline
- **Left/right walls** = Stereo waveforms (guardrails)
- **Camera** = Your position in time and space within the mix

This is one of several experimental UI approaches. The modular design allows for multiple 3D interfaces to coexist.

## Current Status: Waveform Visualization POC

✅ **Basic 3D scene rendering**
- Grid road surface
- Dynamic waveform walls generated from audio data
- Atmospheric lighting (dark cyan/teal aesthetic)
- Clear dark blue background

✅ **Flight camera controls (Descent-style 6DOF)**
- W/A/S/D - Move forward/left/back/right
- Q/E - Move up/down
- Shift - Speed boost (3x)
- Right Mouse + Move - Look around

✅ **Audio → 3D Mesh Pipeline**
- `WaveformData` resource stores loaded audio samples
- Sample de-interleaving (left/right channels)
- Procedural mesh generation from sample data
- Dynamic mesh updates when audio changes
- Test data generator (sine waves for POC)

## Running the Example

```bash
cargo run --example highway -p vvdaw-ui-3d
```

Controls:
- `W/A/S/D` - Move
- `Q/E` - Up/Down
- `Shift` - Speed boost
- `Right Mouse + Move` - Look around
- `Esc` - Exit

## Architecture

### Modules

- `lib.rs` - Main plugin and app setup
- `scene.rs` - Lighting and environment
- `camera.rs` - Flight camera system
- `highway.rs` - Road and wall geometry
- `waveform.rs` - Audio data storage and mesh generation

#### Waveform Pipeline

```
Audio Samples (Vec<f32>)
         ↓
WaveformData Resource
         ↓
De-interleave (L/R channels)
         ↓
generate_channel_mesh()
         ↓
Bevy Mesh (vertices, indices, normals)
         ↓
Rendered as 3D geometry
```

**Key insight**: Samples are cloned from the audio playback thread, ensuring "what you see is what you hear".

### Next Steps

1. **Real Audio File Integration** ⚠️ In Progress
   - ✅ Mesh generation from sample data working (POC with sine waves)
   - ⏸ Wire up to actual WAV file loading from `vvdaw-ui`
   - ⏸ Clone samples from audio thread for visualization
   - ⏸ File picker UI integration

2. **Streaming & Performance**
   - Stream geometry generation as playback progresses (only render visible sections)
   - Dynamic LOD based on camera distance
   - Mesh chunking for infinite highway effect
   - GPU instancing for repeated geometry

3. **Audio Sync**
   - Camera auto-advance with playback position
   - Playback cursor visualization
   - Spatial audio positioning (what you hear depends on where you are)

4. **Atmospheric Effects**
   - Add fog (once we figure out Bevy 0.15 API)
   - Volumetric lighting
   - Particle effects for stereo field visualization
   - Grid shader for road surface

5. **Interaction**
   - Waveform editing in 3D space
   - Time scrubbing via position
   - Visual feedback for audio events
   - Selection/manipulation tools

## Design Philosophy

This UI is **modular and experimental**. It teaches us:
- 3D rendering with Bevy
- Real-time audio → geometry conversion
- Camera navigation patterns
- Performance optimization for streaming geometry

Everything learned here applies to the larger "Fantastic Voyage" vision (multi-scale spatial audio manipulation).

## Visual Style

Per `docs/3d_daw_concept_design.md`:
- Cool neon blues, turquoise, cyan
- Dark environments with grid patterns
- Holographic UI elements
- Soft volumetric lighting

## Future Integrations

### Input Management: `leafwing-input-manager`

**Why**: Much more ergonomic than raw Bevy input handling.

**Current approach** (scattered checks):
```rust
if keyboard.pressed(KeyCode::KeyW) { ... }
if keyboard.pressed(KeyCode::ShiftLeft) { ... }
```

**With leafwing** (action maps):
```rust
enum FlightAction {
    MoveForward,
    MoveLeft,
    Boost,
    LookAround,
    // ...
}
```

**Benefits**:
- Remappable controls
- Multiple input devices (gamepad, HOTAS, MIDI controllers)
- Action states (just pressed, held, released)
- Cleaner system signatures

### Physics: `Avian3d`

**Why**: Enable natural, game-like interaction with audio geometry.

#### Collision Feedback
Subtle haptic/visual responses when navigating:
- **Soft bumps** against waveform walls
  - Camera shake
  - Visual pulse/ripple
  - Audio cue (collision with audio data!)
- **Boundary constraints** - physics-based rails that guide without hard-locking
- **Natural deceleration** near obstacles

#### Magnetic Effects ("Reverse Physics")
The killer feature - audio features can exert forces on the camera:

**Waveform Peak Attraction**
- Peaks act as gravity wells
- Camera naturally "falls" toward interesting transients
- Inverse-square law for smooth, predictable behavior

**Silence Zones Repel**
- Flat/quiet sections gently push camera away
- Guides attention to dynamic content

**Plugin "Monuments" Gravitational Pull**
- Large effect structures (reverb cathedrals, EQ prisms) exert attraction
- Strength proportional to effect intensity/wetness

**Stereo Field Vector Forces**
- Left channel pushes left
- Right channel pushes right
- Stereo width creates lateral forces
- Natural navigation along stereo image

#### Scale Transition Physics
For the larger "Fantastic Voyage" vision:
- **Smooth zoom** between macro/micro scales
- **Inertia preservation** across scale changes
- **Adaptive gravity** - forces scale with zoom level
- **Quantum snapping** - discrete jumps to specific scales (sample level, beat level, bar level)

#### Example Interaction Flow

```
1. User flies toward loud section
2. Waveform peaks attract camera (magnetic pull)
3. Camera accelerates naturally toward peak
4. User can fight the pull or go with it
5. On approach, subtle collision prevents "passing through"
6. Camera orbits the peak if lateral velocity present
7. User can "land" on a peak and inspect that moment in time
```

### Implementation Notes

Both additions are **planned but not required for MVP**:
- Current raw input works fine for initial prototyping
- Physics adds complexity but unlocks truly novel UX
- Can be integrated incrementally (input first, then physics)

The combination enables **emergent navigation patterns** where the audio itself guides exploration - the mix becomes a landscape you can feel, not just see.
