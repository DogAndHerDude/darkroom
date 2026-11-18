//! The keyboard shortcuts list: a dialog over either module, opened and closed with `?`.

use rune::{El, canvas, col, el, row, spacer, text};

use crate::State;
use crate::theme as t;
use crate::ui::cross_icon;

/// What a shortcut does, and its keys.
type Shortcut = (&'static str, &'static [&'static str]);

/// The shortcuts, in groups under a title.
const GROUPS: [(&str, &[Shortcut]); 4] = [
    ("Anywhere", &[
        ("Library", &["G"]),
        ("Develop", &["D"]),
        ("Previous / next photo", &["←", "→"]),
        ("Rate", &["0–5"]),
        ("Pick / reject / unflag", &["P", "X", "U"]),
        ("Copy / paste edits", &["⇧⌘C", "⇧⌘V"]),
    ]),
    ("Develop", &[
        ("Undo / redo", &["⌘Z", "⇧⌘Z"]),
        ("Before and after, side by side", &["Y"]),
        ("Show the original while held", &["\\"]),
        ("Fit / 100%", &["Z"]),
        ("Crop tool", &["C"]),
        ("Put a picker down, or back to Library", &["Esc"]),
    ]),
    ("Library", &[
        ("Open in Develop", &["Return"]),
        ("Add or take out a photo", &["⌘-click"]),
        ("Select a range", &["⇧-click"]),
        ("Select all shown", &["⌘A"]),
    ]),
    ("Crop and export", &[("Leave the crop tool", &["Return", "Esc"]), ("Close the export dialog", &["Esc"])]),
];

pub fn dialog(s: &State) -> Option<El> {
    if !s.shortcuts {
        return None;
    }
    let close = el()
        .key("shortcuts-close")
        .role("button")
        .label("Close")
        .size(28.0, 28.0)
        .center()
        .radius(7.0)
        .hover(t::BG_HOVER)
        .on_press(|s: &mut State| s.shortcuts = false)
        .child(canvas(cross_icon(t::INK_MUTED)).size(20.0, 20.0));
    let head = row()
        .w_full()
        .gap(12.0)
        .child(
            col()
                .gap(4.0)
                .shrink()
                .child(text("Keyboard shortcuts").font(t::DISPLAY).color(t::INK))
                .child(row().gap(4.0).center_y().child(text("Press").font(t::CAPTION).color(t::INK_MUTED)).child(key("?")).child(text("to open or close this list.").font(t::CAPTION).color(t::INK_MUTED))),
        )
        .child(spacer())
        .child(close);
    // Two columns of the dialog's 720 less its padding and the gap, the groups filling them in order.
    let column = |groups: &[(&str, &[Shortcut])]| groups.iter().fold(col().w(328.0).gap(20.0), |c, (title, rows)| c.child(group(title, rows)));
    let body = row().w_full().gap(32.0).child(column(&GROUPS[..2])).child(column(&GROUPS[2..]));
    Some(
        col()
            .key("shortcuts-overlay")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .fill(t::BG_CANVAS.alpha(0.7))
            .child(col().key("shortcuts").w(720.0).padding(16.0).gap(20.0).radius(12.0).fill(t::BG_PANEL).child(head).child(body)),
    )
}

fn group(title: &str, rows: &[Shortcut]) -> El {
    rows.iter().fold(col().w_full().gap(6.0).child(text(title.to_owned()).font(t::LABEL).color(t::INK_MUTED)), |c, (what, keys)| {
        c.child(row().w_full().gap(12.0).center_y().child(text(what.to_string()).font(t::BODY).color(t::INK).shrink()).child(spacer()).children(keys.iter().map(|k| key(k))))
    })
}

/// A key as the design draws it: mono type on a raised chip.
fn key(name: &str) -> El {
    el().px(6.0).py(2.0).radius(4.0).fill(t::BG_RAISED).child(text(name.to_owned()).font(t::VALUE_SM).color(t::INK))
}
