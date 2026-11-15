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

Phase: **implementation.** Every v1 area has a working first version. What follows is what's built, what isn't, what's built but known to be wrong, and what nobody has tried on real files yet. Last brought up to date 2026-11-15.

### Built

| Area | What works |
| --- | --- |
| Preview | GPU develop pipeline drawn inside the RUNE window; Fit or 100% (the top-bar control or `Z`), where the full-size photo is developed and shown pixel for pixel; drag or scroll to pan |
| Edit files | `.darkroom` beside each photo: read on open, written on slider release, removed when the edit is empty |
| Light | Exposure, Brilliance, Highlights, Shadows, Brightness, Contrast, Black point. Brilliance, Highlights and Shadows act on regions, not single pixels |
| Color | Saturation, Vibrance, Temperature, Tint; a neutral-point picker (the eyedropper in the section header) sets Temperature and Tint from a click on the photo |
| Curves | RGB, R, G, B: add, drag and remove points (double-click); live histogram behind; black, grey and white point pickers on the photo. Every picker shows the pixel's R G B beside the pointer; Esc puts it down |
| Detail | Definition, Sharpening, Noise reduction, Vignette; same reach at any render size |
| Sliders | Drag; double-click the track or the label to reset; click the value to type one (Return or a click elsewhere sets it, Esc cancels) |
| Sections | The eye icon hides or shows a section's edits; Reset all |
| Before/after | The original while `\` is held, or toggled with the top-bar button |
| Copy/paste edits | "Copy edits" and "Paste" under the panel in Develop, or ⇧⌘C and ⇧⌘V there and in the Library, where paste goes onto every selected photo. Paste replaces Light, Color, Curves and Detail and keeps each photo's own crop, rating and flag; one undo step a photo |
| Presets | The Presets section at the bottom of Develop's panel: "Save preset…" keeps the photo's Light, Color, Curves and Detail under a typed name (Return saves, Esc cancels; a taken or unusable name says why under the field); a click on a name puts that look on the photo as one undo step, keeping its crop, rating and flag; the preset matching the photo's look is lit; the cross deletes after a confirm. Kept as one TOML file each in `~/Library/Application Support/darkroom/presets`, never overwritten |
| Undo/redo | ⌘Z and ⇧⌘Z, or the arrows in the top bar, in Develop. One step per finished change (a slider drag, a curve point, a crop, a reset); each photo keeps its steps until the app quits |
| Histogram | Live from the GPU, at the top of the panel, with the photo's aperture, shutter, ISO and focal length under it |
| Clipping warnings | The histogram's two squares paint crushed shadows blue and blown highlights red over the photo. Preview only; exports never carry them |
| Edited thumbnails | The grid, the info panel and the filmstrip show an edited photo as edited, crop included; made again when you leave the photo |
| Crop | Aspect presets (Free, Original, 1:1, 4:5, 16:9), straighten ±45°, rotate 90°, flip; applied in preview and export |
| Library | Folder tree, thumbnail grid with disk cache, show All/Picks/Rejects, minimum rating, sort by time or name, thumbnail size, info panel with EXIF |
| Film simulation | The info panel shows a Fujifilm photo's simulation, grain effect, color chrome, dynamic range and white balance, read from the camera's MakerNote (JPEG, or a RAF's embedded JPEG). Shown only; it doesn't change how the photo is developed |
| Multi-select | In the grid: ⌘-click adds a photo or takes it out, ⇧-click selects the range from the photo clicked before, ⌘A selects all shown. Ratings, flags, paste and export act on the whole selection; the info panel shows the photo clicked last |
| Ratings and flags | In the Library and Develop, by click or key |
| Filmstrip | In Develop, with ← → |
| First launch | Choose a root folder; remembered |
| Export | JPEG, PNG, 16-bit TIFF; long-edge resize; sRGB or Display P3 with ICC profile; camera info without location; one photo, the Library's selection, or everything shown; progress and failures listed |
| RAF | Developed with `rawler` to linear floats; thumbnails and EXIF from the embedded JPEG |

### Not built

In the spec or the designs, with no code yet.

| Feature | Design | Notes |
| --- | --- | --- |
| Paste with a choice of sections | ✗ | Paste always takes all four sections |
| Before/after split view | ✗ | The held key and the toggle are built |
| Film simulation under the photo's name in Develop | ✅ | The design's top bar reads "Classic Chrome · 4 of 248"; only the name is shown |
| Folder watching | n/a | New photos appear after reopening the folder |
| Missing folder or drive, unreadable file | ✗ | Failures show as a line of status text only |
| Export file naming | ✅ | Files keep the photo's name; no template |
| Curves luminance-only mode | ✗ | Still "maybe" in the spec |
| Bundled fonts (Rubik, DM Mono) | n/a | The design system ships `.woff2`; unknown whether RUNE loads it |
| Theme generated from `tokens.json` | n/a | `theme.rs` is converted by hand |
| Keyboard shortcut list, settings screen | ✗ | |

Design column: ✅ designed on the canvas · ◐ partly · ✗ not designed · n/a nothing to design.

### Built, but known to be wrong or weak

- **Out-of-gamut colour:** the hard step in saturated blues is gone (largest neighbour step on a generated blue chart went from 38 levels to 3 or less at moderate settings). Hue and lightness are kept in Oklab, but how much chroma survives is read off a straight line in linear RGB, and a small residual is removed along that line, so it is not the nearest in-gamut colour. At Saturation +100 the blue-to-white ramp has a crease in its slope (no step). In Display P3 exports of the same chart the largest step between neighbouring source levels is 6 to 8 levels, where a channel reaches zero at the gamut edge (a steep toe, not the old jump). Generated charts only, not a real photo.
- **100% view:** a second full-size pipeline lives on the GPU while a photo is open at 100% (the source plus three `Rgba16Float` textures; roughly 0.7 GB for 26 MP, a guess from the texture sizes, not measured). Slider speed at 100% is unmeasured. Until the full size has decoded, the preview is shown enlarged. The histogram and pickers still read the 2560 px preview. An unedited photo matches its source exactly, and an edited one (Brilliance, Highlights, Shadows, Definition) matches its full-size sRGB export exactly.
- **Neutral-point picker:** Temperature and Tint only reach ±0.3 and ±0.25 stops, so a strong cast is clamped at ±100 and only partly removed. It reads a 5×5 mean of the source pixels, ignoring the other adjustments.
- **Designs not followed exactly:** Copy edits, Paste and Reset all use the raised button, not the design's quiet one; the before/after button is the small icon button, the design's is the regular size; the clipping squares sit in an 18 pt press area, larger than the design's 10 pt square; the 100% view, the neutral picker in use and the lit eye of a hidden section are not on the canvas (see "Designs still to draw").
- **Local tone is tuned on one synthetic scene:** the base's two numbers (`EPSILON` in `base.wgsl`, how big a brightness step counts as an edge; `base_radius`, the window, 1/60 of the long edge) are guesses set by eye on a generated backlit scene. Real photos may want others.
- **Local tone keeps fine texture, not everything:** detail about the size of the window or larger (broad clouds) counts as region and is still flattened, as the old global curves did. A region pushed more than halfway to black or white has its detail compressed too. Brightness and Contrast stay global.
- **Local tone speed is unmeasured:** five extra quarter-size passes per edit change while Brilliance, Highlights or Shadows are set, and five more for Definition. Not timed on a 26 MP photo or during a drag.
- **Sharpening raises noise** in flat areas (sd 7 → 11 at 80 on the noisy test image). Wants a masking control.
- **Film simulation names are mostly unseen:** the tag numbers and values come from ExifTool's Fujifilm table, and the names are written as the X series' menus have them, from memory ("Shade", "Fluorescent 1", "Auto, ambience priority", "Eterna Bleach Bypass"). Only one camera file was read (an X-T4: Provia, grain off, color chrome off, DR100, auto white balance); every other value was seen only on a copy of that file with the numbers changed by hand. A value not in the table shows no row. Color Chrome FX Blue, a Kelvin white balance's number and the older cameras' portrait modes aren't shown. The info panel's label column is 88 pt, set so "Dynamic range" and the exposure line both fit; a longer lens name or exposure line would run out of the panel.
- **RAF look:** no film simulation, and a base tone fitted to one X-T4 sample. A RAF starts flatter and less saturated than its JPEG.
- **RAF speed:** a few seconds to open (debug build), developed again on every open.
- **RAF hot pixels:** isolated bright pixels at 100%; the camera maps them out, nothing here does.
- **RAF + JPEG pairs** show as two separate photos.
- **Undo history:** lost on quit, and capped at 200 steps a photo. Rating and flag changes aren't steps. The top-bar arrows have no tooltip and their icons don't brighten on hover as the design's do.
- **Before/after drops the crop too:** the original shows whole and unrotated, so a cropped photo jumps in size instead of comparing like with like.
- **Copied edits live in the app only:** not on the system clipboard, gone on quit. In the Library, copy and paste are shortcuts only (no button), and a paste there can only be undone photo by photo, with ⌘Z after opening each one in Develop, and only until the app quits.
- **Multi-select is clicks and ⌘A only:** no ⇧-arrow to extend, no drag to select, no key to drop the selection (click a photo). A selected photo that a filter hides stays selected but is left alone by ratings, flags, paste and export, and comes back selected when the filter is lifted. The filmstrip in Develop doesn't show or use the selection.
- **The Library shows no status line:** a rating, flag or paste that fails there (an unreadable edit file) says so only in Develop's panel, and nothing in the Library confirms a paste except the thumbnails changing. The same goes for a photo the Library refuses to open. An edit file's parse error is several lines long and runs out of the panel's one-line status.
- **Ratings, flags and paste on many photos run in the click or key handler:** each photo's edit file is read and written there, one after another. Untimed beyond six photos.
- **Clipping warnings are a rule of thumb:** highlights warn when any channel shows at 255, shadows only when all three show at 0. A fully saturated colour therefore warns as a highlight (the test chart's hue bars do). The test reads the pixel as drawn, so at Fit a clipped speck smaller than a screen pixel can go unmarked. There is no shortcut key and no count of clipped pixels.
- **Edited thumbnails lag the edit:** a thumbnail is made again when you open another photo or leave Develop, not while you edit, and each time decodes the whole file (a RAF is developed again, a few seconds). A change to the pipeline's code doesn't refresh thumbnails already cached. A photo edited and then reset leaves its old edited thumbnail in the cache, unused.
- **Presets are Develop only and undesigned:** built without a design, in the panel's style. No applying a preset to the Library's selection, no rename, no updating a preset with a new look (delete and save again), no folders of presets. The folder is read at launch and after a save or delete, so a file added in Finder shows on the next launch. A deleted preset is gone (no undo, no Trash). A preset file that doesn't parse is skipped and named in the status line, and only the first such file is named. Preset files are written in full, like edit files. The section sits below Detail, so it is off screen until the panel is scrolled.
- **Verbose edit files:** reading is sparse (missing fields are neutral), but saving writes every field, so a rating alone produces a full file.
- **Preview is 8-bit sRGB:** RUNE has no known path for high-bit-depth or P3 output of app-supplied images. Processing is float and exports are unaffected.
- **TIFF exports carry no camera info.**

### Not yet tried on real files

Everything was built against generated test images and one downloaded X-T4 RAF.

- The native folder picker (headless runs can't open it), for the library root and the export destination
- Real Fujifilm JPEGs: EXIF fields, orientation, speed (the EXIF line was seen only on a JPEG with hand-written EXIF)
- 100% on a real 26 MP JPEG or a RAF: only a generated 5400 × 3600 PNG so far
- Holding `\` on a real keyboard (key repeat, a layout where `\` needs a modifier), ⇧⌘C and ⇧⌘V from real keys, and the before/after button's tooltip: scripted key events only
- Panning with a real trackpad or mouse wheel (scripted drag and wheel events only)
- Local tone (Brilliance, Highlights, Shadows) and Definition on any real photo: only a generated backlit scene so far
- The top bar beside the real traffic lights, and in full screen: checked only against RUNE's headless overlay of where macOS draws them
- Edited thumbnails of a RAF, and of a folder with many edited photos (each is decoded in full first): three generated PNGs so far. The filmstrip's refresh after moving to the next photo wasn't looked at on its own
- Clipping warnings on a real photo and at 100%: the test chart at Fit only
- Film simulation on the user's own cameras and settings: one downloaded X-T4 RAF and its embedded JPEG so far
- ⌘-click, ⇧-click and ⌘A from a real mouse and keyboard: scripted clicks with held modifiers only, on six generated PNGs
- Thumbnail speed on a folder of 26–40 MP files (first visit decodes each one)
- Noise reduction at full size on a noisy high-ISO photo (the taps are spread apart for export; only seen on ISO 160)
- Slider drag speed with noise reduction on (81 taps per pixel)
- Presets from a real keyboard and mouse, and in the real `~/Library/Application Support`: scripted clicks and typing in a scratch home only, on the test chart and one generated PNG
- Exported capture time and lens info as other apps read them
- Display P3 exports viewed on a P3 screen

## Running

```sh
cargo run -p darkroom                            # the folder chosen last time, or asks for one
cargo run -p darkroom -- path/to/folder          # browse a folder (not remembered)
cargo run -p darkroom -- path/to/photo.jpg       # open a photo in Develop (or a .RAF)
cargo run -p darkroom -- --snap out.png --scene edited   # headless snapshot, no window
# DARKROOM_NO_PREFS=1 ignores saved settings (root folder, thumbnail size), for tests
# Develop scenes on the test chart: develop, edited, curves, curve-red, extreme, warm, hidden, original, clipping, definition, sharpen, denoise, crop, cropped, zoom, picking, presets
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

The working texture holds display-referred linear values. The present pass applies the geometry (see "Crop and straighten"), paints the clipping warnings when they are on, and encodes.

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
- **Thumbnails and EXIF** come from the JPEG the camera embeds in every RAF (`darkroom-core/src/raf.rs`), so an unedited RAF shows the camera's rendering in the grid, not Darkroom's. Once edited, its thumbnail is Darkroom's.
- **Speed:** a 26 MP RAF takes a few seconds to open in a debug build. Each open develops it again; nothing is cached.
- **rawler in debug builds:** its demosaic relies on wrapping arithmetic, so the root `Cargo.toml` turns overflow checks off for that one package.

## Library

- **No catalog:** the library is a root folder, chosen on first launch and remembered in RUNE's `Prefs`.
- **Background work:** folder scans, folder loads (EXIF and edit files), thumbnails and opening a photo all run on background threads (`crates/darkroom/src/jobs.rs`). They post messages and wake the window, and `poll` applies them.
- **Thumbnails:** 448 px on the long edge, cached as JPEGs in `~/Library/Caches/darkroom/thumbs`. The cache is keyed by path, size and modification time, so a folder opens instantly the second time.
- **Edited thumbnails:** a photo with adjustments is decoded at thumbnail size on a worker, developed on the GPU the way an export is (`Preview.thumbs`, at most four a frame), and cached as `<key>.edit.jpg` beside `<key>.edit.key`, which names the edit it shows. A different edit replaces both files.
- **Shortcuts:**

  | Key | Does |
  | --- | --- |
  | 0–5 | Rate the photo (in the Library, every selected photo) |
  | P / X / U | Pick / reject / unflag (likewise) |
  | ← → | Previous / next photo |
  | Return | Open the selected photo in Develop |
  | G or Esc | Back to the Library |
  | D | Develop |
  | C | Crop tool on and off (Develop) |
  | ⌘Z / ⇧⌘Z | Undo / redo (Develop) |
  | ⇧⌘C / ⇧⌘V | Copy / paste edits (in the Library, paste goes onto every selected photo) |
  | ⌘-click / ⇧-click / ⌘A | Add or take out a photo / select a range / select all shown (Library) |
  | `\` held | Show the original (Develop) |
  | Z | Fit / 100% (Develop) |
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
- The 100% view: the photo filling the stage, the grab cursor, the "Loading the full size…" note (built without a design)
- The neutral-point picker: its eyedropper in the Color header and the hint above the photo (built to match the curve pickers)
- A hidden section's eye (built as the accent-lit icon; the canvas shows only the visible state)
- Export progress, finished and failed states (built as a plain dialog; wants the Alert component)
- Before/after split view
- The before/after button while the original shows (built as the accent-lit toggle)
- Paste with nothing copied (built as a dimmed Paste button)
- The info panel with several photos selected (built showing the photo clicked last), and copy and paste in the Library (shortcuts only)
- The export dialog's "Selected" option (built into the existing Photos control)
- Clipping overlay on the photo (built as flat `clip-shadow` and `clip-highlight` over the clipped pixels)
- Presets: list, save, apply, delete (built as a section under Detail: name rows, the lit row for the photo's look, an inline name field, an inline delete confirm)
- Paste edits with a choice of sections
- White balance picker in use on the photo
- Error and warning states: unreadable file, missing folder or drive
- RAF-specific UI: true white balance in Kelvin, anything decoder-specific
- RAF + JPEG pairs in the grid
- Keyboard shortcut list; settings, if any are needed
- New icons to add to the design system: eyedropper, folder, flip, swap orientation, cross (deletes a preset)

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
- **Edited thumbnails:** rendered through the develop pipeline, once per edit, when the photo is left.
- **Edge-aware base:** a guided filter (He, Sun, Tang 2010) on quarter-size luminance, guided by itself. One set of passes serves Brilliance, Highlights, Shadows and Definition.

Open:

- **RAF starting look:** a curve per film simulation, or read the camera's settings from the MakerNote, or leave it flat.
- **RAF preview:** cache the developed preview, or show the embedded JPEG first and swap.
- **Exposure headroom:** values over white survive to the output step and are then mapped into SDR. An EDR preview would need a float display path.
- **Thumbnail cache:** it lives in `~/Library/Caches/darkroom/thumbs` with no size limit or cleanup.

## Later (v2+)

HSL / selective color, masks and local adjustments, healing and clone, LUTs, black & white with a channel mixer, lens correction for RAF, virtual copies, HDR/EDR preview.

Not planned: catalog database, Apple Photos integration, HEIC, cloud sync, printing, plugins.
