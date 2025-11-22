# Physical, Cinematic Lighting for a Bevy 0.17 Audio World

This is a practical recipe for getting a **physical, non-retro look** in an open 3D Bevy world where:

- Highways act as tracks
- Billboards represent plugins
- Highway walls render waveforms

All without real-time ray tracing, and staying friendly to a MacBook Pro (M‑series).

---

## 1. Overall Lighting Strategy

Use **PBR + environment lighting + 1–2 hero lights**.

### Environment / Image-Based Lighting (IBL)

- Add an **HDR sky/environment map** that sets the overall mood:
  - Overcast city, dusk, industrial yard, etc.
- This provides:
  - Soft, realistic base light
  - Natural reflections on metal / plastic
- Gives “physically plausible” lighting without actual ray tracing.

### Directional Key Light (Sun)

- Use **one strong directional light** as your “sun” / key light:
  - Angle it low-ish to get **long, dramatic shadows** across highways and waveform walls.
  - Tint it slightly **warm** (subtle yellow/orange).
- Let the HDR environment be slightly **cooler**:
  - This warm–cool contrast reads as cinematic and physical.

### Optional Fill Light

- If shadows are too harsh, add a **low-intensity fill**:
  - A second directional or a large point/spot light.
  - Very low intensity, mostly to lift deep shadows.
- Goal: preserve contrast, avoid “gamey” flatness.

---

## 2. Materials for a “Physical, Not Mario” Look

Aim for **real-world-ish PBR materials**, not flat primaries.

### Highways

- **Asphalt**
  - Base color: dark gray, slightly desaturated
  - Roughness: medium–high (0.6–0.9)
  - Optional: subtle normal map for noise/grain
- **Guard Rails / Barriers**
  - Metal with slight wear:
    - Metallic: 0.5–1.0
    - Roughness: ~0.3–0.6
  - Color: off-white / light gray, not pure white

### Billboards & Plugin Objects

- Think **painted metal and plastic**:
  - Metallic: low–medium (0.0–0.5)
  - Roughness: medium (0.4–0.7)
- Colors:
  - Base palette: muted / industrial tones (charcoal, navy, rust, olive)
  - Accents: brighter, saturated colors for important plugins or controls
- Add:
  - Emissive strips or “status LEDs” with subtle bloom
  - Frames and brackets to give thickness and physical presence

### Waveform Walls

- **Wall Base**
  - Material like painted concrete or composite panel
  - Roughness: medium–high (0.5–0.9)
  - Slightly darker than the waveform color so the waveform pops
- **Waveform Graphic**
  - Either:
    - A mesh extruded slightly from the wall, or
    - A separate quad with a dedicated material
  - Color: brighter, maybe slightly more saturated
  - Roughness: lower than the wall (0.2–0.5)
  - Optional: a touch of **emissive** to make the waveform feel “alive”

Avoid:

- Flat, pure RGB primaries
- Full-metallic everything
- Super-shiny plastic without roughness

---

## 3. Make Stills Read Like Photos

Sell the physicality with **camera and post**, not just lights.

### Camera

- Use a **slightly longer focal length**:
  - Feels more like a 50–85mm lens than a wide FOV game camera.
  - Compresses space a bit, making scenes feel more photographic.
- Add **depth of field** for close-up shots:
  - Focus on a single billboard, highway segment, or waveform wall.
  - Let background and foreground blur slightly.

### Post-Processing

- **Tonemapping**:
  - Use a filmic or ACES-like tonemapper if available.
- **Bloom**:
  - Light bloom on emissive waveform lines, UI accents, plugin indicators.
- **Vignette** (if you implement it):
  - Subtle darkening at the edges to frame the action.
- **Color Grading** (optional)
  - Slightly cool shadows, warm highlights.
  - Or a consistent tint that matches your world’s mood (e.g. sodium-vapor amber, cyberpunk teal-magenta).

This combination pushes the look toward “cinematic stills” instead of “game capture.”

---

## 4. World Design That Supports the Lighting

Shape your geometry so the lighting has something interesting to do.

### Highways

- Long, curving stretches of road:
  - Let the directional sun rake across them and cast long shadows.
- Overpasses, ramps, and split-level roads:
  - Create layered shadows and frames.
- Repeating light poles or structural ribs:
  - Add rhythm and depth to stills.

### Billboards and Plugin Structures

- Cluster billboards at key junctions:
  - Treat them like visual set pieces.
- Give them **thickness and structure**:
  - Frames, supports, catwalks, cables.
- Light them:
  - Direct light from the sun + emissive elements from the billboard content.
  - Let some be in full sun, others partially shadowed by highway geometry.

### Waveform Walls

- Use **varied panel heights** and partial occlusion:
  - Some waveform segments peek around corners or behind columns.
- Break up monotony:
  - Occasional “broken” or glitched panels
  - Different color themes per region, but keep a consistent overall palette.

All of this gives more surfaces and edges for your lighting to play with.

---

## 5. A Simple Bevy-Friendly Preset

Here’s a conceptual “preset” you can aim for in Bevy terms.

### Lights

- **Environment Light / Skybox**
  - HDR texture with subtle variation (clouds, distant city, dusk, etc.).

- **Directional Key Light**
  - Intensity: relatively high (bright sun feel)
  - Color: slightly warm
  - Casts shadows: enabled
  - Angle: low–medium, not noon overhead

- **Optional Fill Light**
  - Color: neutral or slightly cool
  - Intensity: low
  - No shadows or very soft shadows

### Materials

Rough starting points:

- Asphalt:
  - metallic: 0.0
  - roughness: 0.7–0.9
- Guard rails:
  - metallic: 0.5–1.0
  - roughness: 0.3–0.6
- Billboards/plugins:
  - metallic: 0.0–0.5
  - roughness: 0.4–0.7
- Waveform walls:
  - base wall: roughness 0.6–0.9
  - waveform line: roughness 0.2–0.5, small emissive

### Post

- Turn on bloom for emissive bits.
- Use filmic tonemapping or equivalent if available.
- Consider adding a vignette and subtle DOF in your camera setup or via a post pass.

---

## 6. Summary

You don’t need real-time ray tracing to get:

- **Physical, photographic stills**
- A world of **highways as tracks** and **billboards as plugins**
- **Waveform walls** that feel like industrial video panels

The recipe is:

1. PBR + environment lighting + 1–2 hero lights.
2. Real-world-ish materials (asphalt, painted metal, plastic) with plausible roughness/metallic.
3. Cinematic camera and post (DOF, bloom, tonemapper, vignette).
4. Geometry that gives your lights something interesting to hit.

From here, you can iterate by eye: tweak the HDRI, move the sun angle, adjust roughness, and compose shots until it feels like a photograph of a physical audio world rather than a Mario or tank game.
