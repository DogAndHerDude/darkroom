//! Small pieces both modules use, built to the design system: hairlines, buttons, segmented
//! controls, the rating stars and the top bar.

use rune::{Color, El, LineCap, Path, Shape, canvas, el, row, spacer, text};

use crate::theme as t;
use crate::{Module, State};

pub fn rule() -> El {
    el().w_full().h(1.0).fill(t::LINE)
}

pub fn vrule() -> El {
    el().w(1.0).h_full().fill(t::LINE)
}

/// The short divider between groups of controls in a bar.
pub fn divider() -> El {
    el().w(1.0).h(16.0).fill(t::LINE)
}

/// The design system's secondary Button.
pub fn button(key: impl Into<String>, label: impl Into<String>, press: impl Fn(&mut State) + 'static) -> El {
    let label = label.into();
    el().key(key.into())
        .role("button")
        .label(label.clone())
        .h(28.0)
        .px(12.0)
        .center()
        .radius(7.0)
        .fill(t::BG_RAISED)
        .hover(t::BG_HOVER)
        .on_press(press)
        .child(text(label).font(t::BODY).color(t::INK))
}

/// The design system's small IconButton. Off, it is faint and takes no press.
pub fn icon_button(key: &'static str, label: &'static str, icon: fn(Color) -> Shape, enabled: bool, press: fn(&mut State)) -> El {
    let b = el()
        .key(key)
        .role("button")
        .label(label)
        .size(24.0, 24.0)
        .center()
        .radius(7.0)
        .disabled(!enabled)
        .child(canvas(icon(if enabled { t::INK_MUTED } else { t::INK_FAINT })).size(16.0, 16.0));
    if enabled { b.hover(t::BG_HOVER).on_press(press) } else { b }
}

/// The small IconButton as a toggle: accent-tinted while `on`.
pub fn icon_toggle(key: impl Into<String>, label: impl Into<String>, icon: fn(Color) -> Shape, on: bool, press: impl Fn(&mut State) + 'static) -> El {
    el().key(key.into())
        .role("button")
        .label(label.into())
        .checked(on)
        .size(24.0, 24.0)
        .center()
        .radius(7.0)
        .fill(if on { t::ACCENT_SOFT } else { Color::TRANSPARENT })
        .hover(if on { t::ACCENT_SOFT } else { t::BG_HOVER })
        .on_press(press)
        .child(canvas(icon(if on { t::ACCENT_INK } else { t::INK_MUTED })).size(16.0, 16.0))
}

/// The design's eye in a 20 × 20 box: hides a section's edits.
pub fn eye_icon(color: Color) -> Shape {
    let lids = Path::new()
        .move_to([2.5, 10.0])
        .cubic_to([2.5, 10.0], [5.2, 5.0], [10.0, 5.0])
        .cubic_to([14.8, 5.0], [17.5, 10.0], [17.5, 10.0])
        .cubic_to([17.5, 10.0], [14.8, 15.0], [10.0, 15.0])
        .cubic_to([5.2, 15.0], [2.5, 10.0], [2.5, 10.0])
        .close();
    let pupil: Vec<[f32; 2]> = (0..=16).map(|i| {
        let a = std::f32::consts::TAU * i as f32 / 16.0;
        [10.0 + 2.2 * a.cos(), 10.0 + 2.2 * a.sin()]
    }).collect();
    Shape::new().stroke_path(&lids, 1.5, LineCap::Round, color).stroke(pupil, 1.5, color).view_box(20.0, 20.0)
}

/// The design's eyedropper in a 20 × 20 box: picks something from the photo.
pub fn eyedropper_icon(color: Color) -> Shape {
    // The bulb: most of a circle about (14.65, 4.35), closed across the collar.
    let mut bulb: Vec<[f32; 2]> = (0..=8).map(|i| {
        let a = std::f32::consts::PI * (i as f32 / 8.0 - 0.75);
        [14.65 + 1.77 * a.cos(), 4.35 + 1.77 * a.sin()]
    }).collect();
    bulb.extend([[14.2, 7.3], [11.7, 4.8], [13.4, 3.1]]);
    Shape::new()
        .stroke(bulb, 1.5, color)
        .stroke(vec![[11.0, 5.1], [14.9, 9.0]], 1.5, color)
        .stroke(vec![[12.3, 6.2], [5.5, 13.0], [5.1, 13.8], [4.6, 16.0], [6.8, 15.5], [7.6, 15.1], [14.4, 8.3]], 1.5, color)
        .view_box(20.0, 20.0)
}

/// The design's before-and-after mark in a 20 × 20 box: a frame split down the middle.
pub fn before_after_icon(color: Color) -> Shape {
    // A 15 × 12 frame with corners of radius 3; 1.66 is the radius times the circle constant 0.5523.
    let frame = Path::new()
        .move_to([5.5, 4.0])
        .line_to([14.5, 4.0])
        .cubic_to([16.16, 4.0], [17.5, 5.34], [17.5, 7.0])
        .line_to([17.5, 13.0])
        .cubic_to([17.5, 14.66], [16.16, 16.0], [14.5, 16.0])
        .line_to([5.5, 16.0])
        .cubic_to([3.84, 16.0], [2.5, 14.66], [2.5, 13.0])
        .line_to([2.5, 7.0])
        .cubic_to([2.5, 5.34], [3.84, 4.0], [5.5, 4.0])
        .close();
    Shape::new().stroke_path(&frame, 1.5, LineCap::Round, color).stroke(vec![[10.0, 2.5], [10.0, 17.5]], 1.5, color).view_box(20.0, 20.0)
}

/// The design's undo arrow in a 20 × 20 box: an arrowhead at the left of a line that hooks
/// round and back underneath. `mirror` flips it into redo.
fn hook_arrow(mirror: bool, color: Color) -> Shape {
    let at = |[x, y]: [f32; 2]| if mirror { [20.0 - x, y] } else { [x, y] };
    // The hook is a half circle of radius 4.5 about (11.5, 12.5), from the top round to the bottom.
    let mut hook = vec![at([4.5, 8.0])];
    hook.extend((0..=12).map(|i| {
        let a = std::f32::consts::PI * (i as f32 / 12.0 - 0.5);
        at([11.5 + 4.5 * a.cos(), 12.5 + 4.5 * a.sin()])
    }));
    hook.push(at([9.0, 17.0]));
    Shape::new().stroke(vec![at([7.5, 4.5]), at([4.0, 8.0]), at([7.5, 11.5])], 1.2, color).stroke(hook, 1.2, color).view_box(20.0, 20.0)
}

pub fn undo_icon(color: Color) -> Shape {
    hook_arrow(false, color)
}

pub fn redo_icon(color: Color) -> Shape {
    hook_arrow(true, color)
}

/// The design system's Primary button: the one action a screen exists for.
pub fn primary(key: &'static str, label: &'static str) -> El {
    el().key(key).role("button").label(label).h(34.0).px(16.0).center().radius(7.0).fill(t::ACCENT).hover(t::ACCENT_HOVER).child(text(label).font(t::BODY_STRONG).color(t::ON_ACCENT))
}

/// The design system's SegmentedControl: 2–5 short options, one selected.
pub fn segmented<T: Copy + PartialEq + 'static>(key: &'static str, options: &[(T, &'static str)], current: T, set: fn(&mut State, T)) -> El {
    let mut seg = row().self_start().padding(2.0).gap(2.0).radius(9.0).fill(t::BG_RAISED);
    for &(value, label) in options {
        let on = value == current;
        seg = seg.child(
            el().key(format!("{key}-{label}"))
                .role("radio")
                .label(label)
                .checked(on)
                .h(24.0)
                .px(12.0)
                .center()
                .radius(7.0)
                .fill(if on { t::BG_ACTIVE } else { Color::TRANSPARENT })
                .on_press(move |s: &mut State| set(s, value))
                .child(text(label).font(if on { t::BODY_STRONG } else { t::BODY }).color(if on { t::INK } else { t::INK_MUTED })),
        );
    }
    seg
}

/// A five-point star in a 20 × 20 box, filled or outlined.
fn star_shape(filled: bool, color: Color) -> Shape {
    let pts: Vec<[f32; 2]> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 7.2 } else { 3.1 };
            let a = std::f32::consts::PI * (i as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
            [10.0 + r * a.cos(), 10.4 + r * a.sin()]
        })
        .collect();
    let mut closed = pts.clone();
    closed.push(pts[0]);
    let s = Shape::new().stroke(closed, 1.5, color);
    if filled { s.fill(pts, color) } else { s }.view_box(20.0, 20.0)
}

/// One star at `size`: set in `ink`, empty in `ink-faint`.
pub fn star(on: bool, size: f32) -> El {
    canvas(star_shape(on, if on { t::INK } else { t::INK_FAINT })).size(size, size)
}

/// Five stars at `size`, `rating` of them set.
pub fn stars(rating: u8, size: f32) -> El {
    (1..=5).fold(row().gap(1.0), |r, i| r.child(star(i <= rating, size)))
}

/// The top bar's height. The window's traffic lights are centred in it (`main`).
pub const TOP_BAR: f32 = 44.0;

/// The top bar: the module switch, a title, and whatever the module puts on the right. The
/// window has no titlebar of its own, so macOS draws its traffic lights over the bar's left
/// end; the switch starts `inset` points in, after them (`Cx::titlebar_inset`).
pub fn top_bar(s: &State, inset: f32, title: String, right: El) -> El {
    let switch = segmented("module", &[(Module::Library, "Library"), (Module::Develop, "Develop")], s.module, |s, m| s.switch_to(m)).self_center();
    let several = if s.module == Module::Library { s.selection().len() } else { 0 };
    row()
        .key("top-bar")
        .w_full()
        .h(TOP_BAR)
        .pl(inset + 12.0)
        .pr(12.0)
        .gap(12.0)
        .center_y()
        .fill(t::BG_PANEL)
        .child(switch)
        .child(spacer())
        .child(text(title).font(t::BODY_STRONG).color(t::INK))
        .child(spacer())
        .child(right)
        .child(button("export", if several > 1 { format!("Export {several} photos") } else { "Export…".to_owned() }, move |s| {
            // The open photo from Develop; from the Library the selection if it is more than one
            // photo, and the grid otherwise.
            let chosen = several > 1 || (s.module == Module::Develop && s.target().is_some());
            s.export_scope = Some(if chosen { crate::export::Scope::Chosen } else { crate::export::Scope::Shown });
        }))
}
