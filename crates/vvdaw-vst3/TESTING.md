# VST3 Testing Guide

## Examples That Produce AUDIBLE Sound

### 1. `play_vst3` - Real-time Audio Generation

**This plays sound through your speakers!**

```bash
# Play Noises.vst3 (generates white noise - you'll hear it!)
cargo run --example play_vst3 --release
```

You should hear:
- **White noise** coming from your speakers
- Peak levels around **0.3-0.5** in the console
- Plays for **5 seconds** then stops

### 2. `process_wav` - Offline Audio Processing

**This processes a WAV file through a VST3 effect plugin.**

```bash
# Process skillet.wav through ThingsCrusher effect
cargo run --example process_wav --release

# Then play the output to hear the effect:
# macOS:
afplay test_data/output.wav

# Linux:
aplay test_data/output.wav
```

This will:
- Load `test_data/skillet.wav` (your input audio)
- Process it through ThingsCrusher VST3 plugin
- Save to `test_data/output.wav`
- You can then play both files to hear the difference!

### 3. `load_noises` - Simple Plugin Test

```bash
# Tests plugin loading and basic processing
cargo run --example load_noises --release
```

This is a test example - no audible output, but verifies the plugin works.

## Troubleshooting

### "I don't hear anything!"

1. **Check your volume** - make sure speakers are on
2. **Try play_vst3 first** - it's the simplest real-time example
3. **Verify plugin path** - make sure Noises.vst3 exists at `/Library/Audio/Plug-Ins/VST3/Noises.vst3`

### "Peak levels are 0.0000"

This means no audio is being generated. Common causes:
- Effect plugin without input audio (use Noises instead)
- Plugin buses not activated (should be fixed now)
- Plugin not processing correctly

### "It crashes or fails to load"

Check the error message - common issues:
- Plugin file doesn't exist
- Wrong architecture (x86 vs ARM)
- Incompatible plugin version

## Verifying VST3 Integration Works

**Quick verification steps:**

1. Run `play_vst3` - you should **hear noise** from your speakers
2. Check console output shows peak levels like `0.3-0.5` (not 0.0)
3. Run `process_wav` - should create `test_data/output.wav`
4. Play both input and output WAV files to hear the effect

If all these work, VST3 integration is fully functional! 🎉
