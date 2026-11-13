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
| Detail: Definition, Sharpening, Noise reduction | Working (see notes on halos and noise) |
| Library: folder tree, thumbnail grid, filters, sort, thumbnail size, info panel | Working |
| Ratings and flags | Working, in the Library and Develop (keys 0–5, P, X, U) |
| Filmstrip in Develop | Working |
| First launch: choose a folder (remembered) | Working |
| Crop, straighten, rotate, flip | Working: Crop tool in Develop (C), applied in the preview and in export |
| Export: JPEG, PNG, 16-bit TIFF; resize; sRGB or Display P3; camera info | Working: one photo or everything the grid shows, with progress |
| RAF (Fujifilm raw) | Working: developed with `rawler`; see "RAF" for what the starting look is and isn't |

## Running

```sh
cargo run -p darkroom                            # the folder chosen last time, or asks for one
cargo run -p darkroom -- path/to/folder          # browse a folder (not remembered)
cargo run -p darkroom -- path/to/photo.jpg       # open a photo in Develop (or a .RAF)
cargo run -p darkroom -- --snap out.png --scene edited   # headless snapshot, no window
# DARKROOM_NO_PREFS=1 ignores saved settings (root folder, thumbnail size), for tests
# Develop scenes on the test chart: develop, edited, curves, curve-red, extreme, warm, hidden, definition, sharpen, denoise, crop, cropped
# (the detail scenes need a photo with texture and noise to show anything)
# the panel scrolls; for the Curves section in a snapshot add --size 1280x1500
# drive it: --script events.json (clicks and drags in window points, or "key": an element's key)
# script time is virtual: background work (export, raw decoding) needs a late last event to finish in
cargo test --workspace
```

The toolchain is pinned to Rust 1.99.1 (RUNE's), so rustup installs it on the first build.

## Workspace

| Crate | What |
| --- | --- |
| `crates/darkroom` | The app: RUNE UI, state, and the `Preview` compositor that draws the photo |
| `crates/darkroom-core` | The edit model (`Edit`), monotone cubic curves, geometry (crop and straighten as one affine map), edit-file load/save, the library scan, EXIF, and the RAF header. No GPU or UI |
| `crates/darkroom-render` | GPU develop pipeline: `Params::from_edit`, the develop compute pass (`develop.wgsl`) into an `Rgba16Float` texture, and the present pass that draws it into a render pass |

## Processing pipeline

Several compute passes per edit change, in the spec's order. All share `common.wgsl` (colour spaces, encoding, Oklab, output).

| Pass | File | Does | When |
| --- | --- | --- | --- |
| Develop | `develop.wgsl` | Steps 1–6 below, source → stage (linear Rec.2020, unbounded) | Always |
| Denoise | `denoise.wgsl` | Bilateral 9 × 9 blur on encoded RGB, stage → denoised stage | Noise reduction > 0 |
| Pyramid | `pyramid.wgsl` | Encoded luminance at quarter size, Gaussian in two 1-D passes (σ ≈ 1% of the width) | Definition ≠ 0 |
| Finish | `finish.wgsl` | Steps 7–9 below, → working texture | Always |

Per pixel, in order:

1. **Input:** the sRGB source is decoded to linear and converted to linear Rec.2020, the working space.
2. **White balance:** per-channel multipliers, normalized so white keeps its luminance.
3. **Exposure:** a multiplier, 2^stops.
4. **Light:** black point per channel in linear light. Then Brilliance, Highlights, Shadows, Brightness and Contrast as one tone function on perceptually encoded luminance; the channels are scaled by the luminance ratio, so hue holds.
5. **Color:** Saturation and Vibrance scale chroma in Oklab, at constant hue and lightness.
6. **Curves:** composite then per-channel, on display (sRGB-primary) channels in perceptual encoding. Lookup tables come from `Curve::lut`.
7. **Detail:**
   - **Noise reduction:** in its own pass, before the others.
   - **Sharpening:** an unsharp mask on luminance against a 5 × 5 binomial blur, ignoring differences under 0.01 so flat areas sharpen less.
   - **Definition:** midtone-weighted local contrast. It compares the 5 × 5 blur, not the raw pixel, against the wide quarter-size blur, so it doesn't amplify pixel noise.
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

## Crop and straighten

`Geometry` in the edit holds the crop (a rectangle in the upright photo, 0–1), the straighten angle (±45°), quarter turns and flips. `darkroom-core/src/geometry.rs` turns it into one affine map from output position to source position. The preview (`present.wgsl`) and export (`export.wgsl`) both sample the developed photo through that map, so they can't disagree.

Straightening always crops to the largest rectangle of the chosen aspect that stays inside the rotated photo. Dragging the crop frame can't leave the photo either: a drag that would is cut short along its path, one axis at a time, so the frame slides along the edge.

## Export

The "Export…" button exports the open photo from Develop, or everything the grid shows from the Library; the dialog switches between the two. Settings are remembered.

| Setting | Options |
| --- | --- |
| Format | JPEG (quality 70–100), PNG (8-bit), TIFF (16-bit) |
| Long edge | Full size, 4000, 3000 or 2048 px; never enlarges |
| Colour space | sRGB or Display P3, with the macOS ICC profile embedded |
| Metadata | Camera, lens, exposure and capture time (JPEG and PNG). Location is never written, and neither is orientation: the pixels are already upright |
| Save to | `~/Pictures/Darkroom/<folder name>` unless another folder is chosen |

How it runs: one photo at a time. A background thread decodes the full image and reads its edit; the next frame develops it on the GPU in a temporary `Develop` at full size and reads the result back in strips of at most 64 MB (`darkroom-render/src/export.rs`); a thread encodes the file. Files are written as `.partial` and renamed, and an existing name gets ` (2)`, so nothing is overwritten.

## RAF

`crates/darkroom/src/raw.rs` develops a RAF with [`rawler`](https://crates.io/crates/rawler) (LGPL-2.1): black and white levels, demosaicing, the camera's white balance and colour matrix. X-Trans sensors use rawler's Markesteijn demosaic, which rawler's own develop path doesn't use (it's bilinear and visibly softer). The result goes to the GPU as linear half floats (`Source::LinearHalf`) where a JPEG goes as 8-bit sRGB, so highlights and shadows have real latitude.

- **Starting look:** sensor data is about a stop darker than the camera's JPEG. A fixed base tone (gain 1.8 with a soft shoulder over 0.7, on luminance) brings it level. The numbers come from one X-T4 sample against its embedded JPEG. There is no film simulation, so a RAF starts flatter and less saturated than the JPEG.
- **Thumbnails and EXIF** come from the JPEG the camera embeds in every RAF (`darkroom-core/src/raf.rs`), so the grid shows the camera's rendering, not Darkroom's.
- **Speed:** a 26 MP RAF takes a few seconds to open in a debug build. Each open develops it again; nothing is cached.
- **rawler in debug builds:** its demosaic relies on wrapping arithmetic, so the root `Cargo.toml` turns overflow checks off for that one package.

## Library

- **No catalog:** the library is a root folder, chosen on first launch and remembered in RUNE's `Prefs`.
- **Background work:** folder scans, folder loads (EXIF and edit files), thumbnails and opening a photo all run on background threads (`crates/darkroom/src/jobs.rs`). They post messages and wake the window, and `poll` applies them.
- **Thumbnails:** 448 px on the long edge, cached as JPEGs in `~/Library/Caches/darkroom/thumbs`. The cache is keyed by path, size and modification time, so a folder opens instantly the second time.
- **Shortcuts:**

  | Key | Does |
  | --- | --- |
  | 0–5 | Rate the photo |
  | P / X / U | Pick / reject / unflag |
  | ← → | Previous / next photo |
  | Return | Open the selected photo in Develop |
  | G or Esc | Back to the Library |
  | D | Develop |
  | C | Crop tool on and off (Develop) |

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
| RAF (RAW) support | ✗ | Built; no RAF-specific UI |

## Still missing

### Designs

- Detail section, open (Definition, Sharpening, Noise reduction, Vignette)
- Before/after split view, and the 100% zoom view
- Clipping overlay on the photo
- Presets: list, save, apply
- Paste edits with a choice of sections
- White balance picker in use on the photo
- Export progress, finished and failed states: built without a design (a plain dialog); wants the Alert component
- Error and warning states: unreadable file, missing folder or drive
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
- [x] **Full-resolution path for export:** whole image on the GPU, read back in strips. 100% zoom in the preview is still to do.
- [x] **RAF decoder:** `rawler`, with its Markesteijn demosaic for X-Trans.
- [ ] **RAF look:** no film simulation and a base tone fitted to one sample. Wants checking against real files from the camera in use; a per-film-simulation curve, or reading the camera's settings from the MakerNote, would close the gap to the JPEG.
- [ ] **RAF speed:** cache the developed preview, or show the embedded JPEG first and swap when the raw is ready.
- [ ] **RAF hot pixels:** isolated bright pixels show at 100% (the camera maps them out; nothing here does yet).
- [ ] **RAF + JPEG pairs:** both show in the grid as separate photos. An option to stack or hide one would tidy that.
- [ ] **Export of the embedded capture time:** written to EXIF but not checked in other apps yet. Camera info isn't written to TIFF.
- [x] **Adjustment order:** as in "Processing pipeline"; curves run on display channels.
- [x] **Sidecar name and format:** `DSCF1234.JPG.darkroom`, TOML inside (matches the Library screen's "a .darkroom file").
- [x] **GPU readback:** see "Reading results back from the GPU".
- [ ] **Thumbnails show the original,** not the edit. Rendering edited thumbnails needs the develop pipeline on small images, ideally once per saved edit.
- [ ] **Fuji film simulation in the info panel:** needs reading Fujifilm's MakerNote. Wants a real file to check the tag numbers against.
- [ ] **Thumbnail speed on real 40 MP files:** untested here (the test library is 1800 px JPEGs). The first visit decodes every full image once; the cache makes later visits instant.
- [ ] **Sparse edit files:** a rating alone writes the whole edit (every field at its default). Writing only non-default fields would keep the files readable.
- [ ] **Library details still to build:** multi-select (export is one photo or all shown, nothing between), offline-original warnings, folder watching (new photos appear only after reopening the folder).
- [ ] **Gamut edge in pure blues:** pure, fully saturated sRGB blues show a hard step once saturation or vibrance pushes them out of gamut. It's visible on the test chart's blue ramp even at +10 / +24, and as a hard azure-to-blue edge at extremes. Oklab's hue isn't constant around blue, which is a likely cause. Must be fixed before export; deep skies can get close.
- [ ] **Definition halos:** Definition compares against a Gaussian blur, so strong values leave a light or dark rim along high-contrast edges (visible at 70 around a dark shape on sky). An edge-aware base (guided filter or bilateral pyramid) would remove it. The same base would serve local Brilliance, Highlights and Shadows.
- [ ] **Sharpening on noisy photos:** sharpening still raises noise in flat areas (measured sd 7 → 11 at 80 on the noisy test image). A masking control, as Lightroom has, or pairing with noise reduction, would help.
- [ ] **Detail at preview size:** sharpening and noise-reduction radii are in pixels of whatever is being developed. Export doesn't scale them yet, so at full resolution they act on finer detail than the 2560 px preview shows and look weaker. Definition scales with the image and is unaffected.
- [ ] **Detail timing:** not measured yet. Noise reduction is 81 taps per pixel and the most expensive pass; if dragging feels slow on a full preview, run detail at a lower resolution while the pointer is down.
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
