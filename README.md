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

Phase: **implementation.** Every v1 area has a working first version. What follows is what's built, what isn't, what's built but known to be wrong, and what nobody has tried on real files yet. Last brought up to date 2026-11-13.

### Built

| Area | What works |
| --- | --- |
| Preview | GPU develop pipeline drawn inside the RUNE window; fit-to-view only |
| Edit files | `.darkroom` beside each photo: read on open, written on slider release, removed when the edit is empty |
| Light | Exposure, Brilliance, Highlights, Shadows, Brightness, Contrast, Black point. Brilliance, Highlights and Shadows act on regions, not single pixels |
| Color | Saturation, Vibrance, Temperature, Tint |
| Curves | RGB, R, G, B: add, drag and remove points (double-click); live histogram behind; black, grey and white point pickers on the photo |
| Detail | Definition, Sharpening, Noise reduction, Vignette; same reach at any render size |
| Sliders | Drag; double-click the track to reset |
| Sections | "Hide"/"Show" a section's edits; Show original; Reset all |
| Undo/redo | ⌘Z and ⇧⌘Z, or the arrows in the top bar, in Develop. One step per finished change (a slider drag, a curve point, a crop, a reset); each photo keeps its steps until the app quits |
| Histogram | Live from the GPU, at the top of the panel |
| Crop | Aspect presets (Free, Original, 1:1, 4:5, 16:9), straighten ±45°, rotate 90°, flip; applied in preview and export |
| Library | Folder tree, thumbnail grid with disk cache, show All/Picks/Rejects, minimum rating, sort by time or name, thumbnail size, info panel with EXIF |
| Ratings and flags | In the Library and Develop, by click or key |
| Filmstrip | In Develop, with ← → |
| First launch | Choose a root folder; remembered |
| Export | JPEG, PNG, 16-bit TIFF; long-edge resize; sRGB or Display P3 with ICC profile; camera info without location; one photo or everything shown; progress and failures listed |
| RAF | Developed with `rawler` to linear floats; thumbnails and EXIF from the embedded JPEG |

### Not built

In the spec or the designs, with no code yet.

| Feature | Design | Notes |
| --- | --- | --- |
| Copy/paste edits | ◐ buttons only | Choosing which sections to paste isn't designed |
| Presets | ✗ | |
| 100% zoom and panning | ◐ controls only | The preview develops a 2560 px image; 100% needs the full image (export already has that path) |
| Before/after: hold `\`, split view | ◐ toggle only | "Show original" is a button; no key, no split |
| White balance neutral-point picker | ◐ | Curves has its pickers; Color has none |
| Type a value into a slider | ◐ in the slider spec | |
| Eye icon for hiding a section | ✅ | A text toggle stands in |
| Histogram clipping toggles and overlay on the photo | ◐ overlay not drawn | |
| EXIF line under the histogram | ✅ | |
| Pixel readout next to the cursor while picking | ✅ | Needs a probe on pointer move |
| Fuji film simulation in the info panel | ✅ | Needs Fujifilm MakerNote parsing |
| Edited thumbnails | n/a | The grid shows the original (the camera's JPEG for a RAF) |
| Multi-select in the grid | ✗ | Export is one photo or all shown, nothing between |
| Folder watching | n/a | New photos appear after reopening the folder |
| Missing folder or drive, unreadable file | ✗ | Failures show as a line of status text only |
| Export file naming | ✅ | Files keep the photo's name; no template |
| Curves luminance-only mode | ✗ | Still "maybe" in the spec |
| Bundled fonts (Rubik, DM Mono) | n/a | The design system ships `.woff2`; unknown whether RUNE loads it |
| Theme generated from `tokens.json` | n/a | `theme.rs` is converted by hand |
| Keyboard shortcut list, settings screen | ✗ | |

Design column: ✅ designed on the canvas · ◐ partly · ✗ not designed · n/a nothing to design.

### Built, but known to be wrong or weak

- **Gamut edge in pure blues:** fully saturated sRGB blues show a hard step once saturation or vibrance pushes them out of gamut. Visible on the test chart's blue ramp at +10 / +24. Oklab's hue isn't constant around blue, which is a likely cause. This reaches exports; deep skies can get close.
- **Local tone is tuned on one synthetic scene:** the base's two numbers (`EPSILON` in `base.wgsl`, how big a brightness step counts as an edge; `base_radius`, the window, 1/60 of the long edge) are guesses set by eye on a generated backlit scene. Real photos may want others.
- **Local tone keeps fine texture, not everything:** detail about the size of the window or larger (broad clouds) counts as region and is still flattened, as the old global curves did. A region pushed more than halfway to black or white has its detail compressed too. Brightness and Contrast stay global.
- **Local tone speed is unmeasured:** five extra quarter-size passes per edit change while Brilliance, Highlights or Shadows are set, and five more for Definition. Not timed on a 26 MP photo or during a drag.
- **Sharpening raises noise** in flat areas (sd 7 → 11 at 80 on the noisy test image). Wants a masking control.
- **RAF look:** no film simulation, and a base tone fitted to one X-T4 sample. A RAF starts flatter and less saturated than its JPEG.
- **RAF speed:** a few seconds to open (debug build), developed again on every open.
- **RAF hot pixels:** isolated bright pixels at 100%; the camera maps them out, nothing here does.
- **RAF + JPEG pairs** show as two separate photos.
- **Undo history:** lost on quit, and capped at 200 steps a photo. Rating and flag changes aren't steps. The top-bar arrows have no tooltip and their icons don't brighten on hover as the design's do.
- **Verbose edit files:** reading is sparse (missing fields are neutral), but saving writes every field, so a rating alone produces a full file.
- **Preview is 8-bit sRGB:** RUNE has no known path for high-bit-depth or P3 output of app-supplied images. Processing is float and exports are unaffected.
- **TIFF exports carry no camera info.**
- **A broken edit file is ignored when a photo is opened from the command line** (`darkroom photo.jpg`): the photo opens unedited, and the next change overwrites the file. Opening from the Library refuses instead.

### Not yet tried on real files

Everything was built against generated test images and one downloaded X-T4 RAF.

- The native folder picker (headless runs can't open it), for the library root and the export destination
- Real Fujifilm JPEGs: EXIF fields, orientation, speed
- Local tone (Brilliance, Highlights, Shadows) and Definition on any real photo: only a generated backlit scene so far
- Thumbnail speed on a folder of 26–40 MP files (first visit decodes each one)
- Noise reduction at full size on a noisy high-ISO photo (the taps are spread apart for export; only seen on ISO 160)
- Slider drag speed with noise reduction on (81 taps per pixel)
- Exported capture time and lens info as other apps read them
- Display P3 exports viewed on a P3 screen

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

RUNE is pinned to a `dev` rev in the root `Cargo.toml`, with the four crates RUNE patches patched from the same rev. Bump all five together. `wgpu` is pinned to RUNE's version (`~30.0.1`) so our resources live on RUNE's device.

## Workspace

| Crate | What |
| --- | --- |
| `crates/darkroom` | The app: RUNE UI, state, background jobs, raw decoding, export file writing, and the `Preview` compositor that draws the photo |
| `crates/darkroom-core` | The edit model (`Edit`), monotone cubic curves, geometry (crop and straighten as one affine map), undo history, edit-file load/save, the library scan, EXIF, and the RAF header. No GPU or UI |
| `crates/darkroom-render` | GPU develop pipeline: `Params::from_edit`, the compute passes into `Rgba16Float` textures, histogram and probe readback, the present pass that draws into RUNE's render pass, and the full-size export render |

## Edit files

Each photo's edit is `<photo file name>.darkroom` beside it (`DSCF4127.JPG.darkroom`, TOML inside), so a JPEG and a RAF of the same shot keep separate edits. It holds the adjustments, curves, crop, rating and flag. Missing fields read as neutral, a file that doesn't parse stops the photo opening (so it isn't overwritten) and is reported in the status line, and an edit with nothing in it removes its file. Saves go through a temporary file and a rename. Originals are never written.

## Processing pipeline

Several compute passes per edit change, in the spec's order. All share `common.wgsl` (colour spaces, encoding, Oklab, output).

| Pass | File | Does | When |
| --- | --- | --- | --- |
| Develop | `develop.wgsl` | Steps 1–6 below, source → stage (linear Rec.2020, unbounded) | Always |
| Denoise | `denoise.wgsl` | Bilateral 9 × 9 blur on encoded RGB, stage → denoised stage | Noise reduction > 0 |
| Base | `base.wgsl` | The edge-aware base: encoded luminance at quarter size through a guided filter (five small passes), kept as two coefficients a full-size pass turns back into a base with its own luminance | Before Develop when Brilliance, Highlights or Shadows ≠ 0 (of the exposed source); before Finish when Definition ≠ 0 (of the stage) |
| Finish | `finish.wgsl` | Steps 7–9 below, → working texture | Always |

Per pixel, in order:

1. **Input:** the source (8-bit sRGB for a JPEG or PNG, linear half floats for a RAF) is read as linear sRGB-primary light and converted to linear Rec.2020, the working space.
2. **White balance:** per-channel multipliers, normalized so white keeps its luminance.
3. **Exposure:** a multiplier, 2^stops.
4. **Light:** black point per channel in linear light. Then tone on perceptually encoded luminance; the channels are scaled by the luminance ratio, so hue holds. Brilliance, Highlights and Shadows are local: the brightness of the pixel's region (the edge-aware base) moves along the tone curve and the pixel keeps its distance from it, so a lifted shadow keeps its texture. Black and white stay fixed. Brightness and Contrast are global curves.
5. **Color:** Saturation and Vibrance scale chroma in Oklab, at constant hue and lightness.
6. **Curves:** composite then per-channel, on display (sRGB-primary) channels in perceptual encoding. Lookup tables come from `Curve::lut`.
7. **Detail:**
   - **Noise reduction:** in its own pass, before the others.
   - **Reach:** sharpening and noise reduction are defined in preview pixels and scaled by `Develop::detail_scale` (render long edge ÷ preview long edge: 1 in the preview, about 2.4 for a full-size 26 MP export). Sharpening's blur taps move apart by the scale; noise reduction keeps its 9 × 9 taps and spreads them by a whole-pixel stride, so cost per pixel doesn't grow.
   - **Sharpening:** an unsharp mask on luminance against a 5 × 5 binomial blur, ignoring differences under 0.01 so flat areas sharpen less.
   - **Definition:** midtone-weighted local contrast. It compares the 5 × 5 blur, not the raw pixel, against the edge-aware base, so it doesn't amplify pixel noise, and beside a strong edge the base follows the pixel, so there is no halo.
8. **Vignette:** a radial multiplier.
9. **Output:** convert to linear sRGB, or Display P3 for a P3 export. In-range pixels are untouched, so an unedited photo comes out as it went in. Out-of-range pixels are handled in three steps:
   - **Brighter than white:** all three channels are scaled down by the brightest, keeping hue and saturation.
   - **Too saturated for sRGB:** the colour loses chroma at constant Oklab hue and lightness until it fits.
   - **Far past white in luminance:** the colour fades gradually towards white.

The working texture holds display-referred linear values. The present pass applies the geometry (see "Crop and straighten") and encodes.

## Reading results back from the GPU

The histogram and the curve point pickers need numbers from the GPU. `Develop::update` records its own work and submits it before RUNE's frame: the develop pass, the histogram pass (`histogram.wgsl`, atomics over the working texture) and the probe (`develop.wgsl`'s `probe` entry, one pixel up to just before curves). The queue runs in order, so RUNE's draw of the photo later in the frame still sees the new result.

Each result is copied into a `Readback` buffer. A short-lived thread waits on the device, maps the buffer and puts the values in the shared `Results`, then calls RUNE's `Waker`. The app's `poll` picks them up on the next frame. While a readback is in flight its work is held and retried, so dragging a slider never blocks on the GPU.

Two RUNE Metal-backend quirks this works around (worth reporting upstream):

- **Held submissions:** `map_async` can complete before a held submission has run, so the thread does a blocking `poll` before mapping.
- **Workgroup atomics:** `var<workgroup>` atomics come back as zero, so the histogram counts straight into global atomics. Both run correctly on the stock wgpu backend (`RUNE_GPU_BACKEND=wgpu`).

`DARKROOM_DEBUG=1` prints each readback.

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
  | ⌘Z / ⇧⌘Z | Undo / redo (Develop) |
  | Return or Esc | Leave the crop tool; Esc also closes the export dialog |

## Preview path (decided by the spike)

The photo is drawn through a RUNE **compositor and effect slot**, not `El::pixels` or `NativeSurface`:

1. The view places `effect(PHOTO_SLOT)` where the photo goes.
2. The `Preview` compositor gets RUNE's own `wgpu` device, queue and encoder each frame. It uploads the photo once, and records the develop compute pass only when the edit changed.
3. `Renderer::draw_with` calls back at the slot inside RUNE's render pass. `Develop::draw` fits the working texture into the slot's rect there.

So the preview never leaves the GPU, and processing stays in float until the final draw. RUNE's isolated layers are `Rgba16Float`, so a float target may also allow an EDR path later.

## Still missing

"Not built" and "known to be wrong" above are the list of work. Two more lists belong to it.

### Designs still to draw

- Detail section, open (it is built, laid out like the other sections)
- Export progress, finished and failed states (built as a plain dialog; wants the Alert component)
- Before/after split view, and the 100% zoom view
- Clipping overlay on the photo
- Presets: list, save, apply
- Paste edits with a choice of sections
- White balance picker in use on the photo
- Error and warning states: unreadable file, missing folder or drive
- RAF-specific UI: true white balance in Kelvin, anything decoder-specific
- RAF + JPEG pairs in the grid
- Keyboard shortcut list; settings, if any are needed
- New icons to add to the design system: eyedropper, folder, flip, swap orientation

### Decisions

Made:

- **Preview path:** compositor plus effect slot on RUNE's device.
- **Working space:** linear Rec.2020, f32 in the shader, `Rgba16Float` between passes.
- **Adjustment order:** as in "Processing pipeline"; curves run on display channels.
- **Output transform:** constant-hue chroma reduction in Oklab for anything out of range.
- **Edit file:** `DSCF1234.JPG.darkroom`, TOML inside.
- **GPU readback:** a thread per readback, results picked up in `poll`.
- **Full-size export:** the whole image on the GPU, read back in strips.
- **RAF decoder:** `rawler`, with its Markesteijn demosaic for X-Trans.
- **Detail reach:** in preview pixels, scaled per render.
- **Undo model:** per-photo history in memory (`darkroom-core/src/history.rs`), whole edits, nothing in the edit file.
- **Edge-aware base:** a guided filter (He, Sun, Tang 2010) on quarter-size luminance, guided by itself. One set of passes serves Brilliance, Highlights, Shadows and Definition.

Open:

- **RAF starting look:** a curve per film simulation, or read the camera's settings from the MakerNote, or leave it flat.
- **RAF preview:** cache the developed preview, or show the embedded JPEG first and swap.
- **Exposure headroom:** values over white survive to the output step and are then mapped into SDR. An EDR preview would need a float display path.
- **Thumbnail cache:** it lives in `~/Library/Caches/darkroom/thumbs` with no size limit or cleanup.
- **Edited thumbnails:** render through the develop pipeline once per saved edit, or keep showing originals.

## Later (v2+)

HSL / selective color, masks and local adjustments, healing and clone, LUTs, black & white with a channel mixer, lens correction for RAF, virtual copies, HDR/EDR preview.

Not planned: catalog database, Apple Photos integration, HEIC, cloud sync, printing, plugins.
