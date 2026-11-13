//! The Library module: folders on the left, the current folder's photos in a grid, and the
//! selected photo's details on the right. With no folder chosen yet, the first-launch screen.

use darkroom_core::Flag;
use darkroom_core::library::{Entry, Show, Sort};
use rune::{Color, Drag, El, FileDialogOptions, Phase, ScrollSpec, SurfaceFit, Tx, col, el, row, spacer, text};

use crate::theme as t;
use crate::ui::{button, primary, rule, segmented, star, stars, top_bar, vrule};
use crate::{SIDEBAR_WIDTH, State};

/// Thumbnail sizes the size slider runs between, in points.
pub const THUMB_MIN: f32 = 120.0;
pub const THUMB_MAX: f32 = 240.0;

pub fn view(s: &State) -> El {
    let Some(tree) = &s.tree else {
        return col().w_full().h_full().fill(t::BG_CANVAS).child(top_bar(s, String::new(), spacer())).child(rule()).child(welcome(s));
    };

    // Folders.
    let mut folders = col().w_full().gap(2.0).px(8.0);
    for (depth, f) in tree.flatten() {
        let current = s.folder.as_ref() == Some(&f.path);
        let path = f.path.clone();
        let count = if f.photos > 0 { f.photos.to_string() } else { String::new() };
        folders = folders.child(
            row()
                .key(format!("folder-{}", f.path.display()))
                .role("button")
                .label(f.name.clone())
                .w_full()
                .h(28.0)
                .pl(8.0 + 14.0 * depth as f32)
                .pr(8.0)
                .center_y()
                .radius(7.0)
                .fill(if current { t::ACCENT_SOFT } else { Color::TRANSPARENT })
                .hover(if current { t::ACCENT_SOFT } else { t::BG_HOVER })
                .on_press(move |s: &mut State| s.open_folder(path.clone()))
                .child(text(f.name.clone()).font(t::BODY).color(if current { t::ACCENT_INK } else { t::INK }).truncate_fade().shrink())
                .child(spacer())
                .child(text(count).font(t::VALUE_SM).color(t::INK_MUTED)),
        );
    }
    let sidebar = col()
        .key("folders")
        .w(SIDEBAR_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(t::BG_PANEL)
        .child(row().w_full().h(40.0).px(12.0).center_y().child(text("Folders").font(t::HEADING).color(t::INK)).child(spacer()).child(change_folder_button()))
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(folders))
        .child(rule())
        .child(col().w_full().padding(12.0).child(text("Edits are saved next to each photo as a .darkroom file. Originals are never changed.").font(t::CAPTION).color(t::INK_MUTED).w_full().wrap()));

    // Grid.
    let visible = s.visible();
    let folder_name = s.folder.as_ref().and_then(|f| f.file_name()).map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let count = match (s.loading_folder, visible.len(), s.entries.len()) {
        (true, _, _) => "Loading…".to_owned(),
        (_, v, all) if v == all => format!("{all} photos"),
        (_, v, all) => format!("{v} of {all} photos"),
    };
    let toolbar = row()
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(col().child(text(folder_name).font(t::TITLE).color(t::INK)).child(text(count).font(t::CAPTION).color(t::INK_MUTED)))
        .child(spacer())
        .child(segmented("show", &[(Show::All, "All"), (Show::Picks, "Picks"), (Show::Rejects, "Rejects")], s.filter.show, |s, v| s.filter.show = v))
        .child(button("min-rating", min_rating_label(s.filter.min_rating), |s| s.filter.min_rating = (s.filter.min_rating + 1) % 6))
        .child(segmented("sort", &[(Sort::CaptureTime, "Time"), (Sort::Name, "Name")], s.sort, |s, v| s.sort = v))
        .child(size_slider(s.thumb_size));

    let mut grid = row().w_full().flex_wrap().gap(8.0).padding(16.0);
    for &i in &visible {
        grid = grid.child(cell(s, &s.entries[i]));
    }
    let empty = (visible.is_empty() && !s.loading_folder).then(|| {
        let message = if s.entries.is_empty() { "No photos in this folder." } else { "No photos match this filter." };
        col().w_full().padding(48.0).center().child(text(message).font(t::BODY).color(t::INK_MUTED))
    });
    let grid = col().key("grid").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(grid).children(empty);
    let center = col().grow(1.0).shrink().min_w(0.0).h_full().child(toolbar).child(rule()).child(grid);

    let body = row().w_full().grow(1.0).min_h(0.0).child(sidebar).child(vrule()).child(center).child(vrule()).child(info(s));
    col().w_full().h_full().fill(t::BG_CANVAS).child(top_bar(s, String::new(), spacer())).child(rule()).child(body)
}

fn min_rating_label(min: u8) -> String {
    if min == 0 { "Any rating".into() } else { format!("{}+ stars", min) }
}

/// One photo in the grid: thumbnail, name, flag and stars. Click selects; double-click opens it
/// in Develop.
fn cell(s: &State, e: &Entry) -> El {
    let size = s.thumb_size;
    let selected = s.selected.as_ref() == Some(&e.path);
    let mut image = el().w(size - 8.0).h((size - 8.0) * 2.0 / 3.0).radius(4.0).fill(t::BG_PANEL);
    if let Some(img) = s.thumbs.get(&e.path) {
        image = image.pixels_fit(img.pixels(), SurfaceFit::Fit).fill(Color::TRANSPARENT);
    }
    if selected {
        image = image.border(2.0, t::ACCENT);
    }
    let flag = match e.flag {
        Flag::Pick => Some(text("Pick").font(t::CAPTION).color(t::INK)),
        Flag::Reject => Some(text("Reject").font(t::CAPTION).color(t::INK_FAINT)),
        Flag::None => None,
    };
    let edited = e.edited.then(|| el().size(6.0, 6.0).radius(3.0).fill(t::INK_MUTED).label("Edited"));
    let (select, open) = (e.path.clone(), e.path.clone());
    col()
        .key(format!("cell-{}", e.name))
        .role("button")
        .label(e.name.clone())
        .w(size)
        .padding(4.0)
        .gap(4.0)
        .radius(6.0)
        .hover(t::BG_PANEL)
        .opacity(if e.flag == Flag::Reject { 0.5 } else { 1.0 })
        .on_press(move |s: &mut State| s.selected = Some(select.clone()))
        .on_double_press(move |s: &mut State| {
            s.open(open.clone());
            s.module = crate::Module::Develop;
        })
        .child(image)
        .child(
            row()
                .w_full()
                .gap(4.0)
                .center_y()
                .child(text(e.name.clone()).font(t::CAPTION).color(t::INK_MUTED).truncate_fade().shrink())
                .children(edited)
                .child(spacer())
                .children(flag)
                .child(stars(e.rating, 9.0)),
        )
}

/// The selected photo's details, rating and flag.
fn info(s: &State) -> El {
    let panel = col().key("info").w(300.0).h_full().min_h(0.0).fill(t::BG_PANEL).padding(12.0).gap(16.0);
    let Some(e) = s.selected_entry() else {
        return panel.child(text("Select a photo to see its details.").font(t::BODY).color(t::INK_MUTED));
    };
    let mut image = el().w_full().h(184.0).radius(4.0).fill(t::BG_CANVAS);
    if let Some(img) = s.thumbs.get(&e.path) {
        image = image.pixels_fit(img.pixels(), SurfaceFit::Fit);
    }
    let mut rating = row().gap(2.0);
    for n in 1..=5u8 {
        let path = e.path.clone();
        rating = rating.child(
            el().key(format!("rate-{n}"))
                .role("button")
                .label(format!("{n} star{}", if n == 1 { "" } else { "s" }))
                .padding(3.0)
                .radius(5.0)
                .hover(t::BG_HOVER)
                // Clicking the current rating clears it.
                .on_press(move |s: &mut State| {
                    let current = s.entry(&path).map_or(0, |e| e.rating);
                    s.set_rating(&path, if current == n { 0 } else { n });
                })
                .child(star(n <= e.rating, 16.0)),
        );
    }
    let flag_button = |key: &'static str, label: &'static str, flag: Flag, on: bool| {
        let path = e.path.clone();
        el().key(key)
            .role("button")
            .label(label)
            .checked(on)
            .h(24.0)
            .px(10.0)
            .center()
            .radius(7.0)
            .fill(if on { t::ACCENT_SOFT } else { Color::TRANSPARENT })
            .hover(if on { t::ACCENT_SOFT } else { t::BG_HOVER })
            .on_press(move |s: &mut State| s.set_flag(&path, if on { Flag::None } else { flag }))
            .child(text(label).font(t::BODY).color(if on { t::ACCENT_INK } else { t::INK_MUTED }))
    };
    let flags = row()
        .padding(2.0)
        .gap(2.0)
        .radius(9.0)
        .fill(t::BG_RAISED)
        .child(flag_button("flag-pick", "Pick", Flag::Pick, e.flag == Flag::Pick))
        .child(flag_button("flag-reject", "Reject", Flag::Reject, e.flag == Flag::Reject));

    let x = &e.exif;
    let mut details = col().w_full().gap(6.0);
    for (label, value) in [("Taken", x.taken.as_deref().map(taken)), ("Camera", x.camera.clone()), ("Lens", x.lens.clone()), ("Exposure", Some(x.exposure_line()).filter(|l| !l.is_empty()))] {
        if let Some(value) = value {
            details = details.child(row().w_full().gap(8.0).child(text(label).font(t::LABEL).color(t::INK_MUTED).w(72.0)).child(text(value).font(t::LABEL).color(t::INK).shrink()));
        }
    }
    let open = e.path.clone();
    panel
        .child(image)
        .child(col().gap(2.0).child(text(e.name.clone()).font(t::BODY_STRONG).color(t::INK)).child(text(if e.edited { "Edited" } else { "Not edited" }).font(t::CAPTION).color(t::INK_MUTED)))
        .child(row().w_full().center_y().child(rating).child(spacer()).child(flags))
        .child(rule())
        .child(details)
        .child(spacer())
        .child(button("edit-in-develop", "Edit in Develop", move |s| {
            s.open(open.clone());
            s.module = crate::Module::Develop;
        }))
}

/// "2026:11:09 17:42:11" → "9 Nov 2026, 17:42".
fn taken(exif: &str) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (date, time) = exif.split_once(' ').unwrap_or((exif, ""));
    let parts: Vec<&str> = date.split(':').collect();
    let month = parts.get(1).and_then(|m| m.parse::<usize>().ok()).and_then(|m| MONTHS.get(m.wrapping_sub(1)));
    match (parts.first(), month, parts.get(2).and_then(|d| d.parse::<u32>().ok())) {
        (Some(y), Some(m), Some(d)) => format!("{d} {m} {y}, {}", time.get(..5).unwrap_or(time)),
        _ => exif.to_owned(),
    }
}

/// Thumbnail size, a small unipolar slider.
fn size_slider(size: f32) -> El {
    const W: f32 = 96.0;
    let frac = (size - THUMB_MIN) / (THUMB_MAX - THUMB_MIN);
    el().key("thumb-size")
        .role("slider")
        .label("Thumbnail size")
        .w(W)
        .h(16.0)
        .child(el().absolute(0.0, 6.0).w(W).h(4.0).radius(2.0).fill(t::BG_ACTIVE))
        .child(el().absolute(0.0, 6.0).w(frac * W).h(4.0).radius(2.0).fill(t::ACCENT))
        .child(el().absolute(frac * W - 6.0, 2.0).size(12.0, 12.0).radius(6.0).fill(t::INK))
        .on_drag(|s: &mut State, g: &mut Drag, tx: &mut Tx| {
            tx.instant();
            if matches!(g.phase, Phase::Down | Phase::Move) {
                let f = ((g.start_x + g.dx) / g.w.max(1.0)).clamp(0.0, 1.0);
                s.thumb_size = (THUMB_MIN + f * (THUMB_MAX - THUMB_MIN)).round();
            } else {
                let _ = s.prefs.set("thumb_size", &s.thumb_size);
            }
        })
}

fn change_folder_button() -> El {
    el().key("change-folder")
        .role("button")
        .label("Choose a different folder")
        .h(24.0)
        .px(8.0)
        .center()
        .radius(7.0)
        .hover(t::BG_HOVER)
        .on_press_tx(|_: &mut State, tx: &mut Tx| choose_folder(tx))
        .child(text("Change…").font(t::CAPTION).color(t::INK_MUTED))
}

fn choose_folder(tx: &mut Tx) {
    let options = FileDialogOptions { title: "Choose your photos folder".into(), ..Default::default() };
    tx.open_folder(options, |s: &mut State, result, _tx| match result {
        Ok(Some(path)) => s.set_root(path, true),
        Ok(None) => {}
        Err(e) => s.status = format!("Couldn't open the folder picker: {e}"),
    });
}

/// First launch: no folder yet.
fn welcome(s: &State) -> El {
    col()
        .w_full()
        .grow(1.0)
        .center()
        .gap(16.0)
        .child(text("Choose a folder to start").font(t::DISPLAY).color(t::INK))
        .child(text("Darkroom reads your photos straight from a folder. Your edits are saved beside each photo, and the originals are never changed.").font(t::BODY).color(t::INK_MUTED).w(420.0).wrap().align(rune::Align::Center))
        .child(primary("choose-folder", "Choose folder").on_press_tx(|_: &mut State, tx: &mut Tx| choose_folder(tx)))
        .children((!s.status.is_empty()).then(|| text(s.status.clone()).font(t::CAPTION).color(t::INK_MUTED)))
}

#[cfg(test)]
mod tests {
    use super::taken;

    #[test]
    fn capture_time_reads_as_a_date() {
        assert_eq!(taken("2026:11:09 17:42:11"), "9 Nov 2026, 17:42");
        assert_eq!(taken("garbage"), "garbage");
    }
}
