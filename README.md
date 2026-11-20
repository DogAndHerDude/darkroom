# Darkroom

A personal, non-destructive photo editor for Fujifilm JPEGs and the occasional RAF, written in Rust on the [RUNE](https://github.com/anomalyco/rune) GPU UI framework.

Browse a folder, edit with Apple Photos-style adjustments plus proper RGB curves, then export. Originals are never touched: each photo's edits live in a sidecar file next to it.

## References

| What | Where |
| --- | --- |
| v1 feature spec | [Claude Doc](https://claude.ai/artifact/BNANWQoV2ctnippxEAnmz3) |
| v2 spec: HSL and black & white | [Claude Doc](https://claude.ai/artifact/M9ZwpG5JHmXRs4cvaYT77L) |
| Design system ("Darkroom") | [Design system](https://claude.ai/artifact/2AketXsxSQns3vJyx2HrM3): `tokens.json` is the source of truth |
| v1 screens | [Design canvas](https://claude.ai/artifact/6f6zNFTt3oJi4M5i7UsryE) |
| RUNE | https://github.com/anomalyco/rune (pre-release, pin a `dev` git rev) |

## Status

Phase: **implementation.** Every v1 area has a working first version, and v2 has begun with Selective Color and Black & White. What follows is what's built, what isn't, what's built but known to be wrong, and what nobody has tried on real files yet. Last brought up to date 2026-11-20.

### Built

| Area | What works |
| --- | --- |
| Preview | GPU develop pipeline drawn inside the RUNE window; Fit or 100% (the top-bar control or `Z`), where the full-size photo is developed and shown pixel for pixel; drag or scroll to pan |
| Edit files | `.darkroom` beside each photo: read on open, written on slider release, removed when the edit is empty |
| Light | Exposure, Brilliance, Highlights, Shadows, Brightness, Contrast, Black point. Brilliance, Highlights and Shadows act on regions, not single pixels |
| Color | Saturation, Vibrance, Temperature, Tint; a neutral-point picker (the eyedropper in the section header) sets Temperature and Tint from a click on the photo |
| Selective Color | Eight colour bands (Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta), each with Hue, Saturation and Luminance, in Oklab. A swatch per band chooses whose sliders show, with a dot under any band that's moved; the eyedropper in the header chooses the band of a clicked colour. Bands blend smoothly into each other and leave greys alone. Off (dimmed, with a note) while the photo is black and white. [v2 spec](https://claude.ai/artifact/M9ZwpG5JHmXRs4cvaYT77L) |
| Black & White | A switch in the section header; a mixer with one slider per colour band setting how light that colour comes out in grey; Yellow, Orange, Red and Green filter buttons that set the mixer. Every pixel comes out with R = G = B, in the preview and in export. A RAF shot in a monochrome simulation keeps its colour at decode and opens with the switch on, with no edit file; turning it off shows the colour |
| Curves | RGB, R, G, B: add, drag and remove points (double-click); live histogram behind; the RGB curve applies to the channels or to luminance only ("Apply to", on the RGB tab), which keeps hue and saturation; black, grey and white point pickers on the photo. Every picker shows the pixel's R G B beside the pointer; Esc puts it down |
| Detail | Definition, Sharpening, Noise reduction, Vignette; same reach at any render size |
| Sliders | Drag; double-click the track or the label to reset; click the value to type one (Return or a click elsewhere sets it, Esc cancels) |
| Sections | The eye icon hides or shows a section's edits; Reset all |
| Before/after | Split view from the top-bar button or `Y`: the original left of a line across the photo, the edit right of it, both through the crop; drag the line to move it. Holding `\` shows the whole original |
| Copy/paste edits | "Copy edits" and "Paste" under the panel in Develop, or ⇧⌘C and ⇧⌘V there and in the Library, where paste goes onto every selected photo. Paste replaces Light, Color, Selective Color, Black & White, Curves and Detail and keeps each photo's own crop, rating and flag; one undo step a photo |
| Presets | The Presets section at the bottom of Develop's panel: "Save preset…" keeps the photo's Light, Color, Selective Color, Black & White, Curves and Detail under a typed name (Return saves, Esc cancels; a taken or unusable name says why under the field); a click on a name puts that look on the photo as one undo step, keeping its crop, rating and flag; the preset matching the photo's look is lit; the cross deletes after a confirm. Kept as one TOML file each in `~/Library/Application Support/darkroom/presets`, never overwritten |
| Undo/redo | ⌘Z and ⇧⌘Z, or the arrows in the top bar, in Develop. One step per finished change (a slider drag, a curve point, a crop, a reset); each photo keeps its steps until the app quits |
| Histogram | Live from the GPU, at the top of the panel, with the photo's aperture, shutter, ISO and focal length under it |
| Clipping warnings | The histogram's two squares paint crushed shadows blue and blown highlights red over the photo. Preview only; exports never carry them |
| Edited thumbnails | The grid, the info panel and the filmstrip show an edited photo as edited, crop included; made again when you leave the photo |
| Crop | Aspect presets (Free, Original, 1:1, 4:5, 16:9), straighten ±45°, rotate 90°, flip; applied in preview and export |
| Library | Folder tree, thumbnail grid with disk cache, show All/Picks/Rejects, minimum rating, sort by time or name, thumbnail size, info panel with EXIF |
| Library status line | Under the grid: what the last rating, flag, copy or paste did ("Rated 3 photos 4 stars", "Pasted edits onto 3 photos"), or the first photo it failed on and why, with a count of the rest. Gone when another folder opens |
| Develop title | Under the photo's name, its film simulation and place among the photos shown: "Provia · 2 of 3" |
| Keyboard shortcuts | `?` opens a list of them over either module; `?`, Esc or the cross closes it, and other keys do nothing while it is open |
| Missing folder | A folder that isn't there (moved, deleted, or on a drive that isn't connected) shows the design's warning in the grid, naming the drive when it was under `/Volumes`, with "Look again" and "Choose another library folder…". The library folder is looked for every 2 s, so an ejected drive is noticed and a reconnected one comes back by itself. A remembered library folder that is missing at launch opens to the warning, not the first-launch screen |
| Photo that can't be opened | Develop shows why on the stage (the decoder's own words), with "Try again" and "Show in Finder"; the filmstrip marks it; nothing is open meanwhile, so no edit or rating lands on the photo before. ← → and ratings act on it as on any photo |
| RAF white balance | A RAF's Temperature reads in kelvin: its as-shot white comes from the camera's multipliers and colour matrix, the slider moves away from it in mired and Tint off the Planckian locus; a kelvin can be typed; "Shot at 4700 K" and an "As shot" button under the sliders; the neutral picker finds a kelvin and tint. A JPEG keeps the relative −100…+100 |
| RAF + JPEG pairs | A RAF and the JPEG beside it with the same name show as one photo, the RAF, tagged "RAF + JPG" in the grid and counted once in the tree |
| Folder watching | The root and everything under it are watched (FSEvents). Photos added, removed or replaced, and folders added or removed, show up in the tree, grid and filmstrip within about half a second of the last change (at most 2 s into a long copy). A removed photo leaves the selection; a replaced one gets a new thumbnail. Darkroom's own edit files and temporary files don't count as changes |
| Film simulation | The info panel shows a Fujifilm photo's simulation, grain effect, color chrome, dynamic range and white balance, read from the camera's MakerNote (JPEG, or a RAF's embedded JPEG). A RAF starts in the camera's look (below); for a JPEG it is shown only |
| Multi-select | In the grid: ⌘-click adds a photo or takes it out, ⇧-click selects the range from the photo clicked before, ⌘A selects all shown. Ratings, flags, paste and export act on the whole selection; the info panel shows the photo clicked last |
| Ratings and flags | In the Library and Develop, by click or key |
| Filmstrip | In Develop, with ← → |
| First launch | Choose a root folder; remembered |
| Export | JPEG, PNG, 16-bit TIFF; long-edge resize; sRGB or Display P3 with ICC profile; camera info without location; one photo, the Library's selection, or everything shown; progress and failures listed |
| Releases | `scripts/tag.sh` bumps the version, tests, commits and tags; a pushed tag builds a macOS `.dmg`, a Linux AppImage and a Windows `.msi` (plus bare `.tar.gz` and `.zip`) on GitHub Actions and publishes them as a release, each with the design system's app icon (see "Releasing") |
| RAF | Developed with `rawler` to linear floats; thumbnails and EXIF from the embedded JPEG |
| RAF preview cache | A RAF's developed 2560 px preview is kept in `~/Library/Caches/darkroom/previews`, so a RAF opened before opens in under a second (a headless run took 5.2 s the first time, 0.8 s the second, on an X-T4 sample in a debug build) and looks exactly the same (the snapshots were byte for byte equal). A changed file (new size or modification time) or a new app version is developed again. The folder is kept under 2 GB, least recently opened first out |
| RAF starting look | An unedited RAF starts as the camera rendered it: a tone curve and a chroma curve are fitted per photo against the embedded JPEG, so the film simulation, dynamic range and tone settings carry over, and the edit file stays empty. A black-and-white simulation (from the MakerNote) starts with Black & White on, its colour kept for the mixer. Highlights the JPEG clipped roll off under white, so they stay recoverable |

### Not built

In the spec or the designs, with no code yet.

| Feature | Design | Notes |
| --- | --- | --- |
| Paste with a choice of sections | ✅ | Paste always takes all four sections; the design's "Paste…" opens a popover with a switch per section |
| Export file naming | ✅ | Files keep the photo's name; no template |
| Bundled fonts (Rubik, DM Mono) | n/a | The design system ships `.woff2`; unknown whether RUNE loads it |
| Theme generated from `tokens.json` | n/a | `theme.rs` is converted by hand |
| Settings screen | n/a | Nothing needs one yet |

Design column: ✅ designed on the canvas · ◐ partly · ✗ not designed · n/a nothing to design.

### Built, but known to be wrong or weak

- **Out-of-gamut colour:** the hard step in saturated blues is gone (largest neighbour step on a generated blue chart went from 38 levels to 3 or less at moderate settings). Hue and lightness are kept in Oklab, but how much chroma survives is read off a straight line in linear RGB, and a small residual is removed along that line, so it is not the nearest in-gamut colour. At Saturation +100 the blue-to-white ramp has a crease in its slope (no step). In Display P3 exports of the same chart the largest step between neighbouring source levels is 6 to 8 levels, where a channel reaches zero at the gamut edge (a steep toe, not the old jump). Generated charts only, not a real photo.
- **100% view:** a second full-size pipeline lives on the GPU while a photo is open at 100% (the source plus three `Rgba16Float` textures; roughly 0.7 GB for 26 MP, a guess from the texture sizes, not measured). Slider speed at 100% is unmeasured. Until the full size has decoded, the preview is shown enlarged. The histogram and pickers still read the 2560 px preview. An unedited photo matches its source exactly, and an edited one (Brilliance, Highlights, Shadows, Definition) matches its full-size sRGB export exactly.
- **Neutral-point picker:** on a JPEG, Temperature and Tint only reach ±0.3 and ±0.25 stops, so a strong cast is clamped at ±100 and only partly removed. It reads a 5×5 mean of the source pixels, ignoring the other adjustments.
- **RAF white balance is an approximation:** the as-shot white uses the camera's daylight (D65) matrix only, where a DNG converter blends two by temperature; on the six samples it reads 4700–5750 K and leans green (duv +0.001 to +0.017), plausible for their light but not checked against the camera's or another app's numbers. The new balance is applied as per-channel multipliers in linear Rec.2020 after the camera's look is baked in, not in the camera's own space before it. The slider scales (1 mired and 0.0002 duv per unit) are guesses. A RAF edit saved before this with Temperature or Tint set now reads them as kelvin and locus shifts, so its colour changes. The kelvin range ends at 1667 and 25 000 K. The neutral picker's search stops at ±100 on either slider.
- **RAF + JPEG pairs:** only an exact name match pairs (`DSCF0001.RAF` and `DSCF0001.JPG`). Rating, flag, edits and export all go to the RAF; the JPEG is never shown or edited, and an edit file the JPEG already had is ignored. Removing the RAF from disk brings the JPEG back as a photo of its own.
- **Designs not followed exactly:** Copy edits, Paste and Reset all use the raised button, not the design's quiet one; the before/after button is the small icon button, the design's is the regular size; the clipping squares sit in an 18 pt press area, larger than the design's 10 pt square. Drawn after they were built, and differing from them: the split view's line has a round grip in the middle, and its tags sit in a dark pill; the 100% view's "Loading the full size…" note is a pill over the photo with a spinner, where the app puts plain text above it; a hidden section's eye is crossed through, with "Hidden" beside it and its title dimmed, where the app only lights the eye; presets are saved from a + in the section header, the photo's look is marked with a tick, the delete confirm says "Keep" or "Delete", and a taken name reads "There's already a preset with that name."; Paste with nothing copied is the faint quiet button; the Library's info panel with several selected says "3 photos selected" over a stack of them, with "Mixed" values, rating and flag for all, and Paste and Copy buttons, where the app shows the photo clicked last; the export dialog's finished state has a tick and names any file that got " (2)", and failures are an Alert, where the app lists them as red lines.
- **Local tone is tuned on one synthetic scene:** the base's two numbers (`EPSILON` in `base.wgsl`, how big a brightness step counts as an edge; `base_radius`, the window, 1/60 of the long edge) are guesses set by eye on a generated backlit scene. Real photos may want others.
- **Local tone keeps fine texture, not everything:** detail about the size of the window or larger (broad clouds) counts as region and is still flattened, as the old global curves did. A region pushed more than halfway to black or white has its detail compressed too. Brightness and Contrast stay global.
- **Local tone speed is unmeasured:** five extra quarter-size passes per edit change while Brilliance, Highlights or Shadows are set, and five more for Definition. Not timed on a 26 MP photo or during a drag.
- **Sharpening raises noise** in flat areas (sd 7 → 11 at 80 on the noisy test image). Wants a masking control.
- **Film simulation names are mostly unseen:** the tag numbers and values come from ExifTool's Fujifilm table, and the names are written as the X series' menus have them, from memory ("Shade", "Fluorescent 1", "Auto, ambience priority", "Eterna Bleach Bypass"). Only one camera file was read (an X-T4: Provia, grain off, color chrome off, DR100, auto white balance); every other value was seen only on a copy of that file with the numbers changed by hand. A value not in the table shows no row. Color Chrome FX Blue, a Kelvin white balance's number and the older cameras' portrait modes aren't shown. The info panel's label column is 88 pt, set so "Dynamic range" and the exposure line both fit; a longer lens name or exposure line would run out of the panel.
- **RAF look is tone and chroma only:** a simulation's hue shifts aren't fitted. On five downloaded samples (four Provia and a Classic Chrome on X-T3, X-T4, X-T5, X-Pro3; DR100) the middle of the picture comes within about 1–4 of 255 levels of the camera's JPEG at each luminance percentile, and within a few thousandths in Oklab chroma. On a Velvia sample (X100V) reds come out magenta where the camera's go orange, the sky is paler, and shadows sit 6–8 levels high. The deepest highlights come out up to about 10 levels darker than the camera's (the roll-off is gentler than its clipping). No black-and-white, Sepia, DR200 or DR400 sample has been tried; a black-and-white RAF opening with the switch on is tested in unit tests only (params and the camera look's fit), never on a file, and Sepia's tint is lost (it comes out as low colour, untoned). The JPEG must have the raw's proportions, or the fixed base tone (gain 1.8, shoulder 0.7, fitted to one X-T4 sample) stands in, as it does for a RAF with no JPEG. Fujifilm cameras can write the JPEG in Adobe RGB; that case is read as sRGB.
- **RAF speed:** about 4–5 s to open a 26 MP RAF the first time in a debug build, 1.5 s of it the look's fit and apply (measured on an X-T4 sample, whole run including the headless frame). Opened again it comes from the preview cache. 100% and export still develop the RAF in full every time, and so does an edited RAF's thumbnail.
- **RAF preview cache:** each preview is about 26 MB for a 26 MP photo (stored as the half floats themselves, uncompressed, so it reads back exact), so the 2 GB limit holds about 80; the limit is a guess. A preview whose RAF changed or moved stays until the limit pushes it out. A change to how a RAF is developed (`raw.rs`, `camera_look.rs`) must bump `FORMAT` in `previews.rs`, or previews already cached keep showing the old look until the version changes. Writing the preview adds a 26 MB write to the first open, untimed on its own.
- **RAF hot pixels:** isolated bright pixels at 100%; the camera maps them out, nothing here does.
- **Undo history:** lost on quit, and capped at 200 steps a photo. Rating and flag changes aren't steps. The top-bar arrows have no tooltip and their icons don't brighten on hover as the design's do.
- **Curves' luminance mode is undesigned:** the "Apply to: Channels / Luminance" switch is built in the panel's style, without a design, and shows only on the RGB tab. Luminance is scaled in linear light, so a saturated colour pushed up can go past white in one channel and is then brought in by the output step (as Light's tone is). On the test chart's hue bar an S curve shifted hue by at most 0.5° in this mode against 3.8° on the channels; greys come out the same either way. Generated chart only.
- **Holding `\` drops the crop too:** the original shows whole and unrotated, so a cropped photo jumps in size. The split view keeps the crop on both sides.
- **Split view is undesigned and plain:** built without a design (a 2 pt white line with a 16 pt grab area, and "Before" and "After" tags in the slot's top corners). At 100% the line and tags span the stage, not the photo, when the photo is smaller than the stage. The line is placed from the slot's size as laid out the frame before, so the first frame after a layout change can show it in the wrong place (a one-frame headless snapshot does). The clipping warnings paint both sides. No side-by-side or top/bottom layouts, and the line's place is kept only until the app quits. The original is developed into one more `Rgba16Float` texture the size of what is shown: about 35 MB at the 2560 px preview and about 210 MB for a 26 MP photo at 100% (worked out from the texture sizes, not measured).
- **Copied edits live in the app only:** not on the system clipboard, gone on quit. In the Library, copy and paste are shortcuts only (no button), and a paste there can only be undone photo by photo, with ⌘Z after opening each one in Develop, and only until the app quits.
- **Multi-select is clicks and ⌘A only:** no ⇧-arrow to extend, no drag to select, no key to drop the selection (click a photo). A selected photo that a filter hides stays selected but is left alone by ratings, flags, paste and export, and comes back selected when the filter is lifted. The filmstrip in Develop doesn't show or use the selection.
- **The Library's status line says one thing:** the last action's outcome, or its first failure, cut to one line (an edit file's parse error is several lines long; only the first shows). Copy and paste in the Library are still shortcuts only (no button), and the info panel with several selected still shows the photo clicked last, where the design has a summary.
- **Ratings, flags and paste on many photos run in the click or key handler:** each photo's edit file is read and written there, one after another. Untimed beyond six photos.
- **Clipping warnings are a rule of thumb:** highlights warn when any channel shows at 255, shadows only when all three show at 0. A fully saturated colour therefore warns as a highlight (the test chart's hue bars do). The test reads the pixel as drawn, so at Fit a clipped speck smaller than a screen pixel can go unmarked. There is no shortcut key and no count of clipped pixels.
- **Edited thumbnails lag the edit:** a thumbnail is made again when you open another photo or leave Develop, not while you edit, and each time decodes the whole file (a RAF is developed again, a few seconds). A change to the pipeline's code doesn't refresh thumbnails already cached. A photo edited and then reset leaves its old edited thumbnail in the cache, unused.
- **Presets are Develop only and undesigned:** built without a design, in the panel's style. No applying a preset to the Library's selection, no rename, no updating a preset with a new look (delete and save again), no folders of presets. The folder is read at launch and after a save or delete, so a file added in Finder shows on the next launch. A deleted preset is gone (no undo, no Trash). A preset file that doesn't parse is skipped and named in the status line, and only the first such file is named. Preset files are written in full, like edit files. The section sits below Detail, so it is off screen until the panel is scrolled.
- **Folder watching leaves the open photo alone:** a photo open in Develop that is replaced on disk keeps showing the old pixels until it is opened again; one that is deleted stays open, and editing it writes an edit file for a photo that is gone. Edit files changed by another app (a sync, another Mac) aren't picked up until the folder is opened again, since Darkroom's own saves are ignored by name. Every change reads the whole folder again (EXIF and edit file of every photo) and scans the whole tree; untimed on a large folder or tree. A photo still being copied may get no thumbnail until the copy's last change comes in. A watch that can't start says so in Develop's status line only. Only the library folder itself is looked for every 2 s; FSEvents says nothing when a drive under a watch is ejected, so a subfolder that goes with it is noticed only through its library folder. The folder tree doesn't mark a missing folder, where the design does.
- **Linux and Windows builds are macOS-shaped:** exports there carry no ICC profile (it is read from `/System/Library/ColorSync`), so a Display P3 export is mislabelled as untagged; the thumbnail cache and presets go under `$HOME/Library/...` (on Windows, where `HOME` is usually unset, into the temp folder); the top bar leaves room for macOS's traffic lights. Releases are unsigned on every platform. The Windows `.exe` itself carries no icon (the installer, its Start menu shortcut and Add or Remove Programs do), and the installer has no dialogs beyond Windows' own progress window: no licence page, no choice of folder. The AppImage bundles no libraries, so it needs the system's `libxkbcommon` (every desktop session has it) and glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later), from the runner it is built on. The app icon fills its square to the edges, where macOS's own icons leave a margin, so it looks a little larger than its neighbours in the Dock.
- **Verbose edit files:** reading is sparse (missing fields are neutral), but saving writes every field, so a rating alone produces a full file. Selective Color adds a table per band, eight of them, to every file; the v2 spec's example shows them left out at 0, which saving doesn't do yet.
- **Selective Color's numbers are guesses:** the band centres (Oklab hue 29, 60, 105, 142, 195, 258, 294, 328°) come from measuring sRGB's primaries, skin and a sky; the grey fade-out (full effect from chroma 0.04) and Luminance's reach (±0.5 in Oklab lightness's exponent, about ±1.5 stops) were set without a photo. Hue ±100 turns a band 60% of the way to its neighbour's centre, not all the way as the spec proposes: further, the turn falls off faster than hue rises and the hue bar folds into a hard edge (Blue −60 did, at 100%). Two neighbours turning towards each other are both cut back for the same reason. On the test chart's hue bar the largest step between neighbouring pixels with Blue at −60/+50/−30 is 6 levels, a steep ramp where blue meets purple, not a jump.
- **Selective Color and Black & White are undesigned:** built in the panel's style. The swatch row, the header switch (`ui::switch`) and the band colours (`theme::BANDS`, not in the design system) are new. The mixer's sliders are plain, where the spec has a track in the band's colour. The band eyedropper reads the photo as decoded, before any edit, and shows the curve pickers' RGB readout, not a swatch. No dragging on the photo to change a colour.
- **The black-and-white mixer is a guess too:** ±100 multiplies Oklab lightness by 2^±0.75 (about ±2.25 stops), and the four filters' settings are guesses at what each filter does, not measured against film. On a generated sky, foliage, skin and red scene the Red filter took the sky from 140 to 87 and red from 113 to 156, but skin from 185 to 237, lighter than a red filter on film likely makes it. The grey is Oklab lightness, which isn't how the camera's Acros or Monochrome makes its grey; on a RAF shot that way, the start will differ from the camera's JPEG by an unmeasured amount.
- **Preview is 8-bit sRGB:** RUNE has no known path for high-bit-depth or P3 output of app-supplied images. Processing is float and exports are unaffected.
- **TIFF exports carry no camera info.**

### Not yet tried on real files

Everything was built against generated test images and six RAFs downloaded from raw.pixls.us (X-T3, X-T4, X-T5, X-Pro3, X-E4, X100V).

- RAF white balance on the user's own RAFs, against the camera's and another app's kelvin readings; the neutral picker on a real grey card
- A real external drive or card ejected and reconnected (a 300 MB disk image was used), "Show in Finder" and "Choose another library folder…" (the folder picker can't open headless), and `?` from a real keyboard layout where it needs Shift
- The RAF starting look on the user's own RAFs, and on any black-and-white, Sepia, Classic Neg., Eterna, DR200 or DR400 shot

- The native folder picker (headless runs can't open it), for the library root and the export destination
- Real Fujifilm JPEGs: EXIF fields, orientation, speed (the EXIF line was seen only on a JPEG with hand-written EXIF)
- 100% on a real 26 MP JPEG or a RAF: only a generated 5400 × 3600 PNG so far
- Holding `\` on a real keyboard (key repeat, a layout where `\` needs a modifier), ⇧⌘C and ⇧⌘V from real keys, and the before/after button's tooltip: scripted key events only
- Curves' luminance mode on a real photo (skin, foliage, a saturated sky)
- Selective Color and Black & White on any real photo: the test chart and one generated four-colour PNG (scripted eyedropper, switch, Red filter and a JPEG export) only. A RAF shot in Acros or Monochrome: none was available
- The split view's line dragged with a real mouse or trackpad, and the split on a real photo or a RAF, at Fit or 100%: scripted clicks, keys and one drag on the test chart only
- Panning with a real trackpad or mouse wheel (scripted drag and wheel events only)
- Local tone (Brilliance, Highlights, Shadows) and Definition on any real photo: only a generated backlit scene so far
- The top bar beside the real traffic lights, and in full screen: checked only against RUNE's headless overlay of where macOS draws them
- Edited thumbnails of a RAF, and of a folder with many edited photos (each is decoded in full first): three generated PNGs so far. The filmstrip's refresh after moving to the next photo wasn't looked at on its own
- Clipping warnings on a real photo and at 100%: the test chart at Fit only
- Film simulation names on the user's own cameras and settings: six downloaded RAFs (Provia, Classic Chrome, Velvia) so far
- ⌘-click, ⇧-click and ⌘A from a real mouse and keyboard: scripted clicks with held modifiers only, on six generated PNGs
- The RAF preview cache reaching its 2 GB limit, and on the user's own RAFs: one sample opened twice so far
- Thumbnail speed on a folder of 26–40 MP files (first visit decodes each one)
- Noise reduction at full size on a noisy high-ISO photo (the taps are spread apart for export; only seen on ISO 160)
- Slider drag speed with noise reduction on (81 taps per pixel)
- The release workflow on GitHub (it has never run: `v0.0.1` was tagged before it existed), and the Windows build and `.msi` at all: the Windows code type-checks (`cargo check --target x86_64-pc-windows-msvc`) and nothing more. `build.sh` ran on this Mac (the `.dmg` mounted, holding a universal, validly signed app with its icon; the app wasn't opened from it) and in an Ubuntu 24.04 container (aarch64: the AppImage built, extracted, and ran as far as needing a display; the x86_64 one CI makes is untried). `tag.sh` ran against a scratch clone and bare repo, including a failing clippy. The Linux and Windows apps have never been opened on a desktop
- Folder watching with a Finder copy, a card import, an external or network drive, or a large folder: `cp`, `mkdir` and `rm` from a shell on four generated PNGs only
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
# Develop scenes on the test chart: develop, edited, curves, curve-luma, curve-red, extreme, warm, selective, mono, hidden, original, split, clipping, definition, sharpen, denoise, crop, cropped, zoom, picking, presets
# (the detail scenes need a photo with texture and noise to show anything)
# the panel scrolls; for the Curves section in a snapshot add --size 1280x1500
# drive it: --script events.json (clicks and drags in window points, or "key": an element's key)
# script time is virtual: background work (export, raw decoding) needs a late last event to finish in
cargo test --workspace
```

The toolchain is pinned to Rust 1.99.1 (RUNE's), so rustup installs it on the first build.

RUNE is pinned to a `dev` rev in the root `Cargo.toml`, with the four crates RUNE patches patched from the same rev. Bump all five together. `wgpu` is pinned to RUNE's version (`~30.0.1`) so our resources live on RUNE's device.

## Releasing

```sh
scripts/tag.sh --dry-run          # what the next tag would be
scripts/tag.sh                    # bump the patch version, test, commit "Release vX.Y.Z", tag it
scripts/tag.sh minor --push       # bump minor (or major), and push the commit and tag
scripts/build.sh                  # a release build for this machine only, into dist/
```

`tag.sh` needs a clean tree on `main`. It counts up from the newer of `Cargo.toml`'s version and the latest `vX.Y.Z` tag, sets `[workspace.package] version`, refreshes `Cargo.lock`, and runs the tests and clippy (`-D warnings`) before committing; if they fail it puts `Cargo.toml` back and tags nothing. Without `--push` it prints the push command.

A pushed `vX.Y.Z` tag starts `.github/workflows/release.yml`: `scripts/build.sh` runs on a macOS, a Linux and a Windows runner, and the five packages become that tag's GitHub release. The workflow refuses a tag that doesn't match `Cargo.toml`. Run by hand from the Actions tab, it builds the packages as artifacts without a release.

| Platform | Package |
| --- | --- |
| macOS | `darkroom-X.Y.Z-macos-universal.dmg`: `Darkroom.app` beside a link to Applications, for Apple silicon and Intel, macOS 11 or later, ad-hoc signed (no Developer ID, not notarized: Gatekeeper asks on first open) |
| Linux | `darkroom-X.Y.Z-linux-x86_64.AppImage`: runs as it is (`chmod +x`), with a desktop entry and icon inside; needs `libxkbcommon` and glibc 2.39 or newer. `darkroom-X.Y.Z-linux-x86_64.tar.gz`: the bare binary |
| Windows | `darkroom-X.Y.Z-windows-x86_64.msi`: installs into Program Files with a Start menu shortcut; a newer one replaces an older one. `darkroom-X.Y.Z-windows-x86_64.zip`: the bare `darkroom.exe` |

The packaging tools are fetched by `build.sh` on first use, at versions pinned in it: appimagetool 1.9.1 (MIT) and the AppImage runtime, both checked against their SHA-256, and WiX 5.0.2 (MS-RL; a build tool only, nothing of it ships; v6 and later ask for a maintenance-fee EULA). The icons in `assets/icon` are made from the design system's 1024 px app icon by `scripts/icons.sh` (macOS), and committed; run it again if the icon changes. The installer's `UpgradeCode` in `packaging/darkroom.wxs` must never change.

## Workspace

| Crate | What |
| --- | --- |
| `crates/darkroom` | The app: RUNE UI, state, background jobs, raw decoding, export file writing, and the `Preview` compositor that draws the photo |
| `crates/darkroom-core` | The edit model (`Edit`), monotone cubic curves, geometry (crop and straighten as one affine map), undo history, edit-file load/save, the library scan, EXIF, the RAF header, and the camera look fitted from a RAF's JPEG. No GPU or UI |
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
5. **Color:** Saturation and Vibrance scale chroma in Oklab, at constant hue and lightness. Then Selective Color (`darkroom-core/src/bands.rs`): each pixel belongs to the two colour bands either side of its Oklab hue, by a raised cosine, and takes their hue turn, chroma scale and lightness change, faded out towards grey. Or, when black and white is on (it replaces Selective Color), the pixel's Oklab lightness moved by its bands' mixer settings becomes L³ in all three channels, written straight through the output step so the channels stay exactly equal.
6. **Curves:** composite then per-channel, on display (sRGB-primary) channels in perceptual encoding. In the luminance mode the composite curve maps encoded luminance instead and the channels are scaled by the ratio, so hue and saturation hold. Lookup tables come from `Curve::lut`.
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

`DARKROOM_DEBUG=1` prints each readback, and the as-shot white (multipliers, kelvin, duv) of each RAF opened.

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

- **Starting look:** the camera's, fitted per photo against the JPEG it embedded (`darkroom-core/src/camera_look.rs`). The two can't be compared pixel by pixel, because the camera corrects lens distortion and vignetting in its JPEG and the raw is uncorrected. They are compared by distribution instead, over the middle 60% of each, shrunk to 512 px: the raw's n-th percentile of luminance becomes the JPEG's n-th percentile, which gives a tone curve, and the same for Oklab chroma in the midtones gives a chroma curve. A curve and not one scale, because the camera lifts muted colour (foliage) far more than strong colour (a colour chart). The tone curve is applied to luminance as a ratio, so hue holds; past the JPEG's clipping it rolls off towards white. The MakerNote's black-and-white simulations take all the colour out. The result is baked into the decoded pixels, so every slider at 0 is the camera's look. Without a usable JPEG, a fixed base tone stands in (gain 1.8, shoulder over 0.7; fitted to one X-T4 sample, a guess).
- **Thumbnails and EXIF** come from the JPEG the camera embeds in every RAF (`darkroom-core/src/raf.rs`), so an unedited RAF shows the camera's rendering in the grid, not Darkroom's. Once edited, its thumbnail is Darkroom's.
- **Speed:** a 26 MP RAF takes about 4–5 s to open the first time in a debug build. The look's apply runs on every core. The developed preview is then cached (`crates/darkroom/src/previews.rs`), keyed by the file's path, size and modification time and the app's version, so the next open reads it back instead.
- **rawler in debug builds:** its demosaic relies on wrapping arithmetic, so the root `Cargo.toml` turns overflow checks off for that one package.

## Library

- **No catalog:** the library is a root folder, chosen on first launch and remembered in RUNE's `Prefs`.
- **Background work:** folder scans, folder loads (EXIF and edit files), thumbnails and opening a photo all run on background threads (`crates/darkroom/src/jobs.rs`). They post messages and wake the window, and `poll` applies them.
- **Watching:** one recursive FSEvents watch on the root, through [`notify`](https://crates.io/crates/notify) (CC0-1.0). A thread gathers each burst of changes (until 400 ms pass with none, or 2 s at most), drops Darkroom's own files, and posts them as one message; the tree is then scanned again, and the folder shown is read again if the change is in it.
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
  | Y | Before and after, split (Develop) |
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

Everything on the list is on the canvas now (2026-11-18), apart from:

- New icons in the design system: eyedropper, folder, flip and swap orientation (the cross is the system's `close.svg`). The SVGs are drawn as the canvas has them inline; they aren't uploaded yet
- A settings screen, if one is ever needed
- Selective Color and Black & White (v2): both sections, the band swatches and their colours, the header switch, the band eyedropper's readout, and Selective Color dimmed while black and white is on. Built without designs (2026-11-20); the spec's "Designs needed" table lists them

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
- **RAF starting look:** the camera's settings, as rendered into the embedded JPEG: tone and chroma curves fitted per photo, black and white from the MakerNote.
- **Detail reach:** in preview pixels, scaled per render.
- **Undo model:** per-photo history in memory (`darkroom-core/src/history.rs`), whole edits, nothing in the edit file.
- **Edited thumbnails:** rendered through the develop pipeline, once per edit, when the photo is left.
- **RAF preview:** cache the developed preview on disk, bounded by size, rather than show the embedded JPEG first and swap.
- **Selective Color and Black & White** (the v2 spec's open questions, taken at its recommendations since they weren't answered): the mixer works by the eight colour bands, not by red, green and blue; eight bands, not Apple Photos' six; the section is named Selective Color; a RAF shot in black and white opens with the switch on through a missing setting (as the camera shot it), not one written into its edit file.
- **Edge-aware base:** a guided filter (He, Sun, Tang 2010) on quarter-size luminance, guided by itself. One set of passes serves Brilliance, Highlights, Shadows and Definition.

Open:

- **Exposure headroom:** values over white survive to the output step and are then mapped into SDR. An EDR preview would need a float display path.
- **Thumbnail cache:** it lives in `~/Library/Caches/darkroom/thumbs` with no size limit or cleanup.

## Later (v2+)

Masks and local adjustments, healing and clone, LUTs, lens correction for RAF, virtual copies, HDR/EDR preview.

Not planned: catalog database, Apple Photos integration, HEIC, cloud sync, printing, plugins.
