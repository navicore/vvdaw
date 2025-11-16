# vvdaw-ui-3d

Experimental 3D UI for vvdaw - "Infinite Highway" interface.

## Concept

An immersive 3D interface where audio tracks are represented as infinite highways:
- **Road surface** = Timeline
- **Left/right walls** = Stereo waveforms (guardrails)
- **Camera** = Your position in time and space within the mix

This is one of several experimental UI approaches. The modular design allows for multiple 3D interfaces to coexist.

## Current Status: MVP Foundation

✅ **Basic 3D scene rendering**
- Grid road surface
- Static cyan waveform walls (placeholder geometry)
- Atmospheric lighting (dark cyan/teal aesthetic)
- Clear dark blue background

✅ **Flight camera controls (Descent-style 6DOF)**
- W/A/S/D - Move forward/left/back/right
- Q/E - Move up/down
- Shift - Speed boost (3x)
- Right Mouse + Move - Look around

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

### Next Steps

1. **Waveform Integration**
   - Replace static walls with actual audio waveform data
   - Stream geometry generation as playback progresses
   - Dynamic LOD based on camera distance

2. **Audio Sync**
   - Camera auto-advance with playback position
   - Spatial audio positioning (what you hear depends on where you are)

3. **Atmospheric Effects**
   - Add fog (once we figure out Bevy 0.15 API)
   - Volumetric lighting
   - Particle effects for stereo field visualization

4. **Interaction**
   - Waveform editing in 3D space
   - Time scrubbing via position
   - Visual feedback for audio events

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
