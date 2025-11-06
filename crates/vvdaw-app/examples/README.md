# vvdaw Examples

This directory contains examples demonstrating how to use the vvdaw audio engine and plugin system.

## load_vst3

Demonstrates the complete workflow for loading a VST3 plugin and processing audio through it.

### Usage

```bash
cargo run --example load_vst3 -- /path/to/plugin.vst3
```

### What It Demonstrates

1. **VST3 Plugin Loading**: Using `vvdaw_vst3::load_plugin()` to load a plugin from disk
2. **Communication Channels**: Creating lockless channels for UI ↔ Audio thread communication
3. **Audio Engine**: Starting the real-time audio engine with cpal
4. **Plugin Transfer**: Sending a plugin instance to the audio thread safely
5. **Graph Management**: Adding the plugin as a node in the audio processing graph
6. **Event Monitoring**: Receiving events (Started, Stopped, PeakLevel) from the audio thread

### Common VST3 Plugin Locations

- **macOS**: `/Library/Audio/Plug-Ins/VST3/` or `~/Library/Audio/Plug-Ins/VST3/`
- **Windows**: `C:\Program Files\Common Files\VST3\`
- **Linux**: `~/.vst3/` or `/usr/lib/vst3/`

### Example Output

```
=== VST3 Plugin Loading Example ===

Loading plugin from: /Library/Audio/Plug-Ins/VST3/MyPlugin.vst3

[1/5] Loading VST3 plugin...
✓ Loaded: MyPlugin by Vendor Name
  Version: 1.0.0
  Inputs: 2
  Outputs: 2

[2/5] Creating communication channels...
✓ Channels created

[3/5] Starting audio engine...
  Sample rate: 48000 Hz
  Block size: 512 frames
✓ Audio engine started

[4/5] Sending plugin to audio thread...
✓ Plugin sent to audio thread

[5/5] Starting audio playback...
✓ Audio processing started

=== Audio Processing (monitoring for 5 seconds) ===

→ Audio processing STARTED
  Peak [ch 0]: 0.0521
  Peak [ch 0]: 0.0498
  ...

=== Shutting Down ===

[1/2] Stopping audio...
[2/2] Stopping engine...

✓ Example completed successfully!
```

## Architecture Overview

The example demonstrates the core vvdaw architecture:

```
┌─────────────┐                    ┌──────────────┐
│  UI Thread  │                    │ Audio Thread │
│             │                    │              │
│  load_vst3  │                    │    cpal      │
│     ↓       │                    │   callback   │
│ Vst3Plugin  │──plugin_tx────────→│              │
│             │                    │  AudioGraph  │
│  Commands   │──command_tx───────→│   ↓          │
│             │                    │  Plugin.process()
│             │←──event_rx─────────│              │
│   Events    │                    │              │
└─────────────┘                    └──────────────┘
```

### Key Components

1. **Plugin Loading** (UI thread):
   - Load VST3 binary with `libloading`
   - Call `GetPluginFactory()`
   - Query plugin information
   - Initialize plugin with sample rate and block size

2. **Communication**:
   - **plugin_tx/rx**: `crossbeam::channel` for transferring `Box<dyn Plugin>` instances
   - **command_tx/rx**: `rtrb` ring buffer for commands (AddNode, Start, Stop, etc.)
   - **event_tx/rx**: `rtrb` ring buffer for events (Started, Stopped, PeakLevel, etc.)

3. **Audio Processing** (audio thread):
   - Receive plugin via `plugin_rx.try_recv()` (non-blocking)
   - Add to `AudioGraph` with automatic initialization
   - De-interleave cpal buffers → process graph → re-interleave output
   - Send peak levels and events back to UI

### Real-Time Safety

The audio callback follows strict real-time guidelines:

- ✅ No allocations (buffers pre-allocated)
- ✅ No blocking operations (`try_recv`, not `recv`)
- ✅ No locks (uses lockless channels)
- ✅ Graceful degradation (drops events if queue is full)
- ✅ Plugin initialization happens before adding to graph

## Testing Without a Real Plugin

If you don't have a VST3 plugin available, the audio engine will fall back to generating a 440 Hz test tone. You can still run the example to see the architecture in action:

```bash
# This will fail at plugin loading but demonstrates the error handling
cargo run --example load_vst3 -- /nonexistent.vst3
```

## Next Steps

For a complete DAW application, you would:

1. Add a UI for browsing and selecting plugins
2. Implement parameter automation and MIDI event routing
3. Add support for multiple nodes and connections
4. Implement save/load for sessions and plugin states
5. Add visualization (waveforms, spectrum, etc.)
