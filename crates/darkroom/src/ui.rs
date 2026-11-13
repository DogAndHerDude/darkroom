//! Small pieces both modules use, built to the design system: hairlines, buttons, segmented
//! controls, the rating stars and the top bar.

use rune::{Color, El, Shape, canvas, el, row, spacer, text};

use crate::theme as t;
use crate::{Module, State};

pub fn rule() -> El {
    el().w_full().h(1.0).fill(t::LINE)
}

pub fn vrule() -> El {
    el().w(1.0).h_full().fill(t::LINE)
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

/// The top bar: the module switch, a title, and whatever the module puts on the right.
pub fn top_bar(s: &State, title: String, right: El) -> El {
    let switch = segmented("module", &[(Module::Library, "Library"), (Module::Develop, "Develop")], s.module, |s, m| s.switch_to(m));
    row()
        .key("top-bar")
        .w_full()
        .h(44.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .fill(t::BG_PANEL)
        .child(switch)
        .child(spacer())
        .child(text(title).font(t::BODY_STRONG).color(t::INK))
        .child(spacer())
        .child(right)
}
