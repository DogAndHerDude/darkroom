# Darkroom

A personal, non-destructive photo editor for Fujifilm JPEGs and the occasional RAF, written in Rust on the [RUNE](https://github.com/anomalyco/rune) GPU UI framework.

Browse a folder, edit with Apple Photos-style adjustments plus proper RGB curves, then export. Originals are never touched: each photo's edits live in a sidecar file next to it.

## References

| What | Where |
| --- | --- |
| v1 feature spec | [Claude Doc](https://claude.ai/artifact/BNANWQoV2ctnippxEAnmz3) |
| Design system ("Darkroom") | [Design system](https://claude.ai/artifact/2AketXsxSQns3vJyx2HrM3): `tokens.json` is the source of truth |
| v1 screens | [Design canvas](https://claude.ai/artifact/6f6zNFTt3oJi4M5i7UsryE) |
| RUNE | https://github.com/anomalyco/rune (pre-release, pin a `dev` git rev) |

## Status

Phase: **implementation.** Spec, design system and initial screens are done.

| Area | State |
| --- | --- |
| GPU preview in a RUNE window | Working |
| Edit model and edit files (`.darkroom` beside each photo) | Working: loads on open, saves on slider release |
| Light: Exposure, Brilliance, Highlights, Shadows, Brightness, Contrast, Black point | Working (global tone; see notes) |
| Color: Saturation, Vibrance (Oklab), Temperature, Tint | Working |
| Curves: processing and editor (RGB, R, G, B) | Working: add, drag, remove points; live histogram behind; edited channels shown faintly on RGB |
| Curves: black, grey and white point pickers | Working: pick, then click the photo |
| Histogram at the top of the panel | Working, live from the GPU; no clipping toggles or EXIF line yet |
| Vignette | Working |
| Hide a section's edits | Working (text toggle; eye icon to come) |
| Show original, Reset all | Working |
| Definition, Sharpening, Noise reduction | Not started (need multi-pass shaders) |
| Crop, straighten, rotate, flip | Stored in the edit; not applied yet |
| Library, filmstrip, histogram, export, RAF | Not started |

## Running

```sh
cargo run -p darkroom -- path/to/photo.jpg      # no path: a generated test chart
cargo run -p darkroom -- --snap out.png --scene edited   # headless snapshot, no window
# scenes: edited, curves, curve-red, extreme, warm, hidden
# the panel scrolls; for the Curves section in a snapshot add --size 1280x1500
# drive it: --script events.json (clicks and drags in window points)
cargo test --workspace
```

The toolchain is pinned to Rust 1.99.1 (RUNE's), so rustup installs it on the first build.

## Workspace

| Crate | What |
| --- | --- |
| `crates/darkroom` | The app: RUNE UI, state, and the `Preview` compositor that draws the photo |
| `crates/darkroom-core` | The edit model (`Edit`), monotone cubic curves, and edit-file load/save. No GPU or UI |
| `crates/darkroom-render` | GPU develop pipeline: `Params::from_edit`, the develop compute pass (`develop.wgsl`) into an `Rgba16Float` texture, and the present pass that draws it into a render pass |

## Processing pipeline

`crates/darkroom-render/src/develop.wgsl`, one compute pass per edit change, in the spec's order:

1. **Input:** the sRGB source is decoded to linear and converted to linear Rec.2020, the working space.
2. **White balance:** per-channel multipliers, normalized so white keeps its luminance.
3. **Exposure:** a multiplier, 2^stops.
4. **Light:** black point per channel in linear light. Then Brilliance, Highlights, Shadows, Brightness and Contrast as one tone function on perceptually encoded luminance; the channels are scaled by the luminance ratio, so hue holds.
5. **Color:** Saturation and Vibrance scale chroma in Oklab, at constant hue and lightness.
6. **Curves:** composite then per-channel, on display (sRGB-primary) channels in perceptual encoding. Lookup tables come from `Curve::lut`.
7. **Detail:** not yet.
8. **Vignette:** a radial multiplier.
9. **Output:** convert to linear sRGB. In-range pixels are untouched, so an unedited photo comes out as it went in. Out-of-range pixels are handled in three steps:
   - **Brighter than white:** all three channels are scaled down by the brightest, keeping hue and saturation.
   - **Too saturated for sRGB:** the colour loses chroma at constant Oklab hue and lightness until it fits.
   - **Far past white in luminance:** the colour fades gradually towards white.

The working texture holds display-referred linear sRGB; the present pass only encodes it.

## Reading results back from the GPU

The histogram and the curve point pickers need numbers from the GPU. `Develop::update` records its own work and submits it before RUNE's frame: the develop pass, the histogram pass (`histogram.wgsl`, atomics over the working texture) and the probe (`develop.wgsl`'s `probe` entry, one pixel up to just before curves). The queue runs in order, so RUNE's draw of the photo later in the frame still sees the new result.

Each result is copied into a `Readback` buffer. A short-lived thread waits on the device, maps the buffer and puts the values in the shared `Results`, then calls RUNE's `Waker`. The app's `poll` picks them up on the next frame. While a readback is in flight its work is held and retried, so dragging a slider never blocks on the GPU.

Two RUNE Metal-backend quirks this works around (worth reporting upstream):

- **Held submissions:** `map_async` can complete before a held submission has run, so the thread does a blocking `poll` before mapping.
- **Workgroup atomics:** `var<workgroup>` atomics come back as zero, so the histogram counts straight into global atomics. Both run correctly on the stock wgpu backend (`RUNE_GPU_BACKEND=wgpu`).

`DARKROOM_DEBUG=1` prints each readback.

Edit files are `<photo name>.darkroom` (`DSCF4127.JPG.darkroom`, TOML inside), so a JPEG and RAF of the same shot keep separate edits. They're sparse: missing fields read as neutral. An edit with nothing in it removes its file.

RUNE is pinned to a `dev` rev in the root `Cargo.toml`, with the four crates RUNE patches patched from the same rev. Bump all five together. `wgpu` is pinned to RUNE's version (`~30.0.1`) so our resources live on RUNE's device.

## Preview path (decided by the spike)

The photo is drawn through a RUNE **compositor and effect slot**, not `El::pixels` or `NativeSurface`:

1. The view places `effect(PHOTO_SLOT)` where the photo goes.
2. The `Preview` compositor gets RUNE's own `wgpu` device, queue and encoder each frame. It uploads the photo once, and records the develop compute pass only when the edit changed.
3. `Renderer::draw_with` calls back at the slot inside RUNE's render pass. `Develop::draw` fits the working texture into the slot's rect there.

So the preview never leaves the GPU, and processing stays in float until the final draw. RUNE's isolated layers are `Rgba16Float`, so a float target may also allow an EDR path later.

## Architecture (from the spec)

- **Sidecar edits:** each photo gets a recipe file with adjustments, crop, rating and flag. Originals are read-only.
- **Processing pipeline:** decode → linear wide-gamut float working space → adjustments in a fixed order → output transform (display or export).
- **Proposed adjustment order:** white balance → exposure → light (tone, black point) → color → curves → detail → vignette. Still to confirm.
- **GPU:** adjustments are wgpu compute shaders on the same device RUNE uses. Preview at screen resolution; full resolution only for 100% zoom and export.
- **Design tokens:** generate a Rust constants module from the design system's `tokens.json` at build time, so values aren't copied by hand.
- **Fonts:** bundle Rubik and DM Mono from the design system's `fonts/`.

## v1 features

Design column: ✅ designed on the canvas · ◐ partly designed · ✗ not designed yet.

### Editing

| Feature | Design | Notes |
| --- | --- | --- |
| Light: Exposure, Brilliance, Highlights, Shadows, Brightness, Contrast, Black point | ✅ | Develop screen |
| Color: Saturation, Vibrance, Temperature, Tint | ✅ | Spectrum tracks for Temperature and Tint only |
| Neutral-point white balance picker | ◐ | Button in the Color header; picking state shown only for the Curves grey point |
| Curves: RGB plus R, G, B; monotone cubic; add and remove points | ✅ | Curves screen |
| Curves: black, grey and white point pickers | ✅ | |
| Curves: luminance-only mode | ✗ | Still "maybe" in the spec |
| Detail: Definition, Sharpening, Noise reduction, Vignette | ✗ | Section exists but is collapsed in every screen |
| Crop with aspect presets, straighten, rotate 90°, flip | ✅ | Crop screen |
| Hide a section's edits (eye icon) | ✅ | |
| Double-click a label to reset a slider; click the value to type it | ◐ | Behaviour is in the design system's slider spec, not drawn |

### Viewing and workflow

| Feature | Design | Notes |
| --- | --- | --- |
| Library: folder tree, thumbnail grid, filters, sort, thumbnail size | ✅ | Library screen |
| Ratings and pick/reject flags | ✅ | |
| Info panel with EXIF and Fuji film simulation fields | ✅ | Needs MakerNote parsing |
| Filmstrip in Develop | ✅ | |
| Histogram with clipping toggles | ✅ | Clipping overlay on the photo not drawn |
| Fit and 100% zoom, panning | ◐ | Controls drawn; 100% view not drawn |
| Before/after (hold `\`) and split view | ◐ | Toggle drawn; split view not drawn |
| Undo/redo history | ◐ | Buttons only |
| Copy/paste edits | ◐ | Buttons only; choosing which panels to paste not designed |
| Presets | ✗ | Not designed |
| Export: format, quality, sRGB/P3, resize, metadata, destination, naming, batch | ✅ | Export dialog |
| First launch / empty library | ✅ | |
| RAF (RAW) support | ✗ | Last in line for v1 |

## Still missing

### Designs

- Detail section, open (Definition, Sharpening, Noise reduction, Vignette)
- Before/after split view, and the 100% zoom view
- Clipping overlay on the photo
- Presets: list, save, apply
- Paste edits with a choice of sections
- White balance picker in use on the photo
- Export progress, export finished and export failed states (Alert component)
- Error and warning states: unreadable file, missing folder or drive, RAF not supported yet
- RAF-specific UI: true white balance in Kelvin and anything decoder-specific
- Keyboard shortcut list
- Settings, if any are needed (default export folder, cache location)

### Decisions to make before or during implementation

- [x] **Preview path in RUNE:** compositor plus effect slot on RUNE's device (see "Preview path" above).
- [x] **Output transform:** constant-hue chroma reduction in Oklab for anything out of range (see "Processing pipeline").
- [ ] **Local tone for Brilliance, Highlights and Shadows.** They're global tone curves for now. Apple's versions act locally (they lift a dark area without flattening it), which needs a blurred luminance pass. That pass would also serve Definition.
- [ ] **Exposure headroom.** Values brighter than white survive until output but are then mapped into SDR. Pulling an overexposed photo down already works; an EDR preview would need a float display path.
- [ ] **Design tokens:** `crates/darkroom/src/theme.rs` is converted by hand from `tokens.json` for now; replace it with a build-time generated module.
- [ ] **Fonts:** bundle Rubik and DM Mono. The design system ships `.woff2`; check whether RUNE's font stack loads WOFF2 or needs TTF/OTF.
- [ ] **Full-resolution path:** the spike develops a 2560 px preview only. 100% zoom and export need the full image, likely in tiles.
- [ ] **RAF decoder:** which Rust crate handles X-Trans demosaicing well.
- [x] **Adjustment order:** as in "Processing pipeline"; curves run on display channels.
- [x] **Sidecar name and format:** `DSCF1234.JPG.darkroom`, TOML inside (matches the Library screen's "a .darkroom file").
- [x] **GPU readback:** see "Reading results back from the GPU".
- [ ] **Gamut edge at extremes:** at about +1.5 EV with +100 saturation, pure synthetic blues show a hard edge between azure and blue. Real photos are unlikely to hit it; revisit the gamut mapping when export lands.
- [ ] **Picker hover readout:** the design shows the pixel's values next to the cursor while picking. Needs a probe on pointer move.
- [x] **Working colour space and precision:** linear Rec.2020, f32 in the shader, `Rgba16Float` between passes.
- [ ] **Thumbnail cache:** location and size.
- [ ] **Histogram while Curves is open:** hide it or keep it (the Curves screen currently hides it).
- [ ] **New icons:** add the eyedropper, folder, flip and swap-orientation icons drawn for the screens to the design system.

### Known constraint

RUNE has no known path for high-bit-depth or Display P3 output of app-supplied images, so the v1 preview is likely 8-bit sRGB. Processing stays in float, and exports are unaffected.

## Later (v2+)

HSL / selective color, masks and local adjustments, healing and clone, LUTs, black & white with a channel mixer, lens correction for RAF, virtual copies, HDR/EDR preview.

Not planned: catalog database, Apple Photos integration, HEIC, cloud sync, printing, plugins.
