# Darkroom

Guidance for any model working in this repository. Read it fully before changing code.

## How to work here

These are the working rules for this project. They matter more than speed.

### Scope and decisions

- Do what was asked, at the size it was asked. Don't add features, refactors or abstractions nobody requested. If you spot something adjacent worth doing, mention it; don't do it.
- When you have enough information to act, act. Don't ask questions you can answer by reading the code, the README or the design artifacts. Ask only when the decision is really the user's (a product choice, a trade-off with no sensible default).
- Don't re-open decisions the user has made (listed under "Decisions already made"). If one looks wrong, say so once and carry on with it.
- The project runs in phases the user set: features, design system, designs, implementation. Don't build UI that has no design without saying it has none, and record it in the README under "Designs still to draw".

### Honesty about results

- Never say something works unless you ran it and saw it work. Say what you checked and how. If you couldn't check something, say that plainly.
- If tests fail, report the failure with the output. If you skipped a step, say so.
- A number fitted to one sample is a guess. Label it as one, in the code comment and in the README (see the RAF base tone for the pattern).
- Never invent RUNE API. RUNE is pre-release and under-documented; methods you expect often don't exist (`.p()`, `TextAlign` and `child_limit` were all wrong guesses). Read RUNE's source before using anything not already used in this repo. The pinned rev is in the root `Cargo.toml`; cargo keeps a checkout under `~/.cargo/git/checkouts/`.

### Safety

- Commit or push only when the user asks. The user commits and pushes themselves. If you do commit, end the message with `Co-Authored-By: Claude <noreply@anthropic.com>`.
- Originals are never written. Darkroom only ever writes `.darkroom` files, thumbnails in the cache, and exports to a destination folder. Any change that could write to a photo is a bug.
- Before deleting or overwriting anything, look at what it is. Exports must never overwrite (`unique_path`).
- Test files, scripts and snapshots go in a scratch directory outside the repo, never in the project tree.
- Tests that run the app must set `DARKROOM_NO_PREFS=1`, and `HOME` to a scratch directory if they export or make thumbnails, so they don't touch the user's real settings, cache or `~/Pictures`.

### Code style

- Match the surrounding code: its naming, comment density and idiom. Comments say why, or state a constraint; they don't narrate what the next line does.
- Doc comments are short plain sentences. Module headers (`//!`) explain what the module is for and the one or two ideas a reader needs.
- Long lines are the existing style (there is no rustfmt config; don't run `cargo fmt` over files, it would reflow everything). Long builder chains go one call per line.
- Tests have sentence-like names (`names_never_overwrite`, `the_base_tone_keeps_channel_ratios`) and sit in a `tests` module at the bottom of the file.
- `cargo clippy --workspace --all-targets` must be clean. Fix the lint; don't `#[allow]` it.
- No new dependency without a reason stated to the user, including its licence if it isn't MIT/Apache.

### Communication

- Use they/them for the user. Don't infer pronouns from a name.
- Reports are short and lead with what changed and what state it is in, then what is unverified or still open. No padding, no praise.
- Keep `README.md` current. It is the user's notes file. Its "Status" section has four lists: "Built", "Not built", "Built, but known to be wrong or weak" and "Not yet tried on real files". Every feature change updates it in the same piece of work: move the feature between lists, and put any gap you find or leave in the list it belongs to. Only claim "Built" for what you ran.

## The project

Darkroom is the user's personal Lightroom-like photo editor for macOS, written in Rust on the RUNE GPU-rendered UI framework (https://github.com/anomalyco/rune). It edits their own Fujifilm photos: mostly JPEG, sometimes RAF. It is a learning and enjoyment project, so clarity of the code matters as much as the feature list.

Priorities, in the user's words: RGB curves, and the Apple Photos-style basics (exposure, brilliance, highlights, shadows, black point, saturation and so on).

### References

| What | Where |
| --- | --- |
| Feature notes, status, known gaps | `README.md` (read this next) |
| v1 feature spec | https://claude.ai/artifact/BNANWQoV2ctnippxEAnmz3 |
| Design system "Darkroom" (`tokens.json` is the source of truth) | https://claude.ai/artifact/2AketXsxSQns3vJyx2HrM3 |
| v1 screens | https://claude.ai/artifact/6f6zNFTt3oJi4M5i7UsryE |

### Decisions already made

- No catalog database. The library is a root folder; edits live beside each photo.
- Edit files are named `<photo file name>.darkroom` (`DSCF4127.JPG.darkroom`), TOML inside, sparse (missing fields are neutral), removed when the edit is empty.
- No Apple Photos integration, no HEIC, no cloud sync.
- One dark theme. Fonts are Rubik and DM Mono; accent `#e2a27a`.
- Working space is linear Rec.2020 in `Rgba16Float`; curves run on display (sRGB-primary) channels; out-of-gamut colour is brought in by reducing Oklab chroma (the amount comes from a straight line in linear RGB; searching along the Oklab chroma line made a step in blues).
- RAF is decoded with `rawler`, using its Markesteijn demosaic for X-Trans.

### Workspace

| Crate | What | Depends on |
| --- | --- | --- |
| `crates/darkroom-core` | Edit model, curves, geometry, undo history, sidecars, library scan and filters, EXIF, the Fujifilm look (MakerNote), RAF header | No GPU, no UI. Keep it that way |
| `crates/darkroom-render` | The GPU develop pipeline (WGSL compute passes), readback, present, export render | core, wgpu |
| `crates/darkroom` | The app: RUNE UI, state, background jobs, raw decoding, file export | both |

Where things live in the app crate:

| File | Holds |
| --- | --- |
| `main.rs` | `State`, `Msg` handling, `init`/`setup`/`poll`/`on_key`, the `Preview` compositor, headless scenes |
| `develop.rs`, `curves.rs`, `crop.rs`, `presets.rs` | The Develop module's panels and tools |
| `library.rs` | The Library module |
| `export.rs` | Export settings, dialog, and file writing |
| `jobs.rs` | Background threads, the thumbnail cache (edited thumbnails are developed in the compositor), and the watch on the root folder |
| `photo.rs`, `raw.rs` | Decoding a photo into `Pixels` (8-bit sRGB or linear half float) |
| `ui.rs`, `theme.rs` | Shared controls; colour and type tokens |

### Build

- Rust is pinned (`rust-toolchain.toml`, RUNE's version). Don't change it independently of RUNE.
- RUNE is pinned to a `dev` rev with `features = ["native-file-dialog"]`, and four crates are patched from the same rev (winit, parley_engine, wgpu-hal, accesskit_windows). Bump all five together. `wgpu` is pinned to RUNE's version so our resources live on RUNE's device.
- Dependencies build at `opt-level = 2` in dev (image decoding is unusable otherwise). `rawler` builds at 3 with overflow checks off: its demosaic relies on wrapping arithmetic and panics with checks on.

## Things you need to know to change the code

### RUNE

- Elm-style: `view(&State, &mut Cx<State>) -> El`. Builders are `el() col() row() text() spacer() canvas(shape) effect(mark)`.
- Handlers: `.on_press(|s| ..)`, `.on_press_tx(|s, tx| ..)`, `.on_double_press`, `.on_drag(|s, &mut Drag, &mut Tx| ..)` with `Drag { phase, x, y, w, h, start_x, start_y, dx, dy, .. }`.
- `Color` is **linear**. `theme.rs` holds tokens already converted from the design system's sRGB hex. Never pass sRGB values as a `Color`, and never hard-code a colour outside `theme.rs`.
- It is `.padding()`, not `.p()`. Text alignment is `.align(rune::Align::Center)`. Scrolling needs `.scroll(..)` and `.min_h(0.0)` on its ancestors.
- A local variable named `canvas`, `text`, `row` and so on shadows the builder of that name. Name locals differently.
- Give every interactive element a `.key("...")`. Headless scripts target elements by key. `ui::segmented(key, ..)` gives each option the key `"<key>-<label>"`.
- File dialogs: `tx.open_folder(FileDialogOptions { .. }, |s, result, tx| ..)`. Settings persist through `rune::prefs::Prefs` (`get`/`set`).
- Closures in the `scenes` list need the annotation `|s: &mut State|`.

### Frame and threading model

- Nothing slow runs in `view`, a handler, or `poll`. Decoding, scanning, thumbnails and file encoding go through `jobs.rs`: a thread does the work, posts a `Msg` to the inbox and calls the waker; `poll` drains the inbox and `State::handle` applies it.
- GPU work happens in the `Preview` compositor (`impl Compositor<State>`), which gets RUNE's device, queue and encoder each frame. The photo is drawn at the `effect(PHOTO_SLOT)` element through `f.ui.draw_with`.
- `Develop::update` submits its own encoder before RUNE's frame, so the draw later in the frame sees the new result.
- Readbacks (histogram, picker probe, export) never block the frame: a short-lived thread waits, maps and posts the result.

Two RUNE Metal-backend bugs are worked around. Don't remove the workarounds without testing on Metal:

- `map_async` can complete before a held submission has run, so the readback thread does a blocking `device.poll` first.
- `var<workgroup>` atomics read back as zero, so the histogram counts into global atomics.

`RUNE_GPU_BACKEND=wgpu` runs on the stock backend, to tell our bugs from RUNE's.

### Pipeline

Order: white balance, exposure, light (tone on 2.2-encoded luminance; Brilliance, Highlights and Shadows follow an edge-aware base of the region, `base.wgsl`), colour (Oklab), curves (1024-entry LUT per channel), detail (bilateral denoise, the same base for Definition, unsharp mask), vignette, output transform (sRGB or Display P3). The README has the pass table.

- Adding an adjustment touches, in order: `Edit` in `core/edit.rs` (with a serde default so old sidecars still load), `Params::from_edit` in `render/params.rs`, the `Params` struct in `common.wgsl` (layouts must match exactly), the shader pass, and the slider list in `develop.rs`. Add a test for the neutral value: every adjustment at 0 must leave pixels exactly unchanged.
- The source texture is either `Rgba8UnormSrgb` (JPEG, PNG) or `Rgba16Float` linear (raw). Either way the shader loads linear sRGB-primary values.
- Detail reach (sharpening, noise reduction) is in preview pixels. Anything rendering at another size sets `Develop::detail_scale`; a new detail tool with a pixel radius must honour `p.output.y` and stay bit-identical at scale 1.
- Geometry (crop, straighten, rotate, flip) is one affine map from output to source, built in `core/geometry.rs`. The preview and export both sample through it. Never apply geometry anywhere else.
- Colour lessons already paid for: saturation by RGB mixing shifts hue (use Oklab chroma); curves on Rec.2020 channels shift hue (use sRGB-primary channels); over-bright values are scaled by the max channel and burned towards white by luminance, not clipped per channel.

### Export

One photo at a time: a job decodes the full image and its edit; the next frame builds a temporary `Develop`, renders, and reads back in strips of at most 64 MB; a thread encodes. Files are written as `.partial` and renamed. Location (GPS) is never written, and neither is orientation.

## Testing

Run all three before reporting work as done:

```sh
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets
```

Unit tests cover logic (curves, geometry, sidecars, filters, params, file writing). Anything visual or GPU-side needs the headless run.

### Headless snapshots

The app renders a frame to a PNG with no window. Look at the PNG; a clean exit proves nothing.

```sh
export DARKROOM_NO_PREFS=1 HOME=/path/to/scratch/home
./target/debug/darkroom --snap out.png --scene edited                 # a named scene on the test chart
./target/debug/darkroom path/to/photo.jpg --snap out.png --size 1280x800
./target/debug/darkroom path/to/folder --snap out.png --script events.json
```

- Scenes: `develop, edited, curves, curve-red, extreme, definition, sharpen, denoise, warm, crop, cropped, hidden, original, clipping, zoom, picking, presets`. Add a scene when you add a state worth looking at.
- The right-hand panel scrolls; use `--size 1280x1500` to see the lower sections.
- `DARKROOM_DEBUG=1` prints each GPU readback.

Script events:

```json
[
  {"t": 2.0, "type": "click", "key": "export"},
  {"t": 2.5, "type": "click", "x": 76, "y": 158},
  {"t": 3.0, "type": "drag", "x": 400, "y": 300, "dx": 80, "dy": 0, "dur": 0.5},
  {"t": 4.0, "type": "type", "text": "3"},
  {"t": 4.3, "type": "key", "name": "Right"},
  {"t": 60.0, "type": "move", "x": 5, "y": 5}
]
```

- `"key"` on a click is an element key. A keyboard key is `{"type": "key", "name": "Right"}`; typed characters are `{"type": "type", "text": ".."}`. Return is named `Enter`; there are also `double-click` and `wheel` (`dx`, `dy`) events.
- Script time is virtual and runs faster than real time. Background work (decoding, export, thumbnails) needs real time, so put the last event late (tens of seconds, 100+ for a RAF export), or the run ends before the work does.
- A click on a key that doesn't exist yet aborts the run. If a button appears only after background work, schedule the click well after it.

### What to check for each kind of change

| Change | Check |
| --- | --- |
| An adjustment or shader | Snapshot at neutral (must match the untouched photo), at a moderate value and at the extreme; look for hue shifts, banding and speckle |
| Geometry or crop | `crop` and `cropped` scenes, a scripted drag, and an export of a cropped, straightened photo compared with the preview |
| Export | Output size, bit depth and profile (`sips -g pixelWidth -g pixelHeight -g bitsPerSample -g profile -g make -g model file`), no `.partial` left behind, a second export gets ` (2)` |
| Library | A folder with subfolders and some `.darkroom` files; filters, sort, rating keys via script |
| RAF | A real RAF (samples at https://raw.pixls.us/data/Fujifilm/); compare against its embedded JPEG |

The user tests the native folder picker and their own camera files by hand; headless runs can't open a native dialog.

## Known gaps

`README.md` under "Status" has the full lists. The ones most likely to bite:

- At 100% a second, full-size `Develop` lives in the compositor (`Preview.full`); its memory and slider speed are unmeasured.
- An edited photo's thumbnail is made again only when the photo is left, and a pipeline change doesn't refresh thumbnails already cached (delete `~/Library/Caches/darkroom/thumbs/*.edit.*`).
- The RAF starting look is a base tone fitted to one X-T4 sample, with no film simulation.
- Presets work in Develop only, with no design yet. No before/after split yet; paste takes all sections. The Library has no status line, so a failure there shows only in Develop's panel.
