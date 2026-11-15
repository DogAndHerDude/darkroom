//! The Develop panel's Presets section: saved looks to put on the open photo with one click.
//!
//! Presets live as files in `~/Library/Application Support/darkroom/presets` (see
//! `darkroom_core::preset`). Applying one is a paste of its look, one undo step. No design yet.

use std::path::PathBuf;

use darkroom_core::preset::{self, Preset};
use rune::{Color, El, Key, KeyEvent, Tx, canvas, col, el, row, spacer, text, text_input};

use crate::State;
use crate::theme as t;
use crate::ui::{button, cross_icon, rule};

/// Where presets are kept: beside the app's other files, not in the library.
pub fn dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, |h| PathBuf::from(h).join("Library/Application Support")).join("darkroom/presets")
}

impl State {
    /// Reads the presets folder again. A file that can't be read is left alone and named in
    /// the status line.
    pub fn load_presets(&mut self) {
        let (presets, failed) = preset::load_all(&self.presets_dir);
        self.presets = presets;
        if let Some(first) = failed.first() {
            self.status = format!("Couldn't read preset {first}");
        }
    }

    /// Puts the named preset's look on the open photo.
    pub fn apply_preset(&mut self, name: &str) {
        let Some(look) = self.presets.iter().find(|p| p.name == name).map(|p| p.look.clone()) else { return };
        self.put_look(&look);
    }

    /// Opens the name field for a new preset of the open photo's look.
    fn start_preset(&mut self) {
        let mut field = rune::TextEdit::new("");
        field.focused = true;
        (self.preset_name, self.naming_preset, self.preset_deleting) = (field, true, None);
    }

    /// Saves the open photo's look as a preset under the typed name. A refused name keeps the
    /// field open; the field says why.
    pub fn save_preset(&mut self) {
        match preset::save(&self.presets_dir, &self.preset_name.text, &self.edit, &self.presets) {
            Ok(saved) => {
                self.status = format!("Saved preset “{}”", saved.name);
                self.naming_preset = false;
                self.load_presets();
            }
            Err(preset::Error::Name(_)) => {}
            Err(e) => self.status = format!("Couldn't save the preset: {e}"),
        }
    }

    fn delete_preset(&mut self, name: &str) {
        self.preset_deleting = None;
        match preset::delete(&self.presets_dir, name) {
            Ok(()) => self.status = format!("Deleted preset “{name}”"),
            Err(e) => self.status = format!("Couldn't delete the preset: {e}"),
        }
        self.load_presets();
    }
}

/// The section: the presets by name, the one matching the photo's look lit, and the button
/// that saves the look as a new one.
pub fn section(s: &State) -> El {
    let look = s.edit.look();
    let mut list = col().w_full().gap(2.0);
    if s.presets.is_empty() {
        list = list.child(text("No presets yet. Save this photo's look to use it on others.").font(t::LABEL).color(t::INK_MUTED).wrap());
    }
    for p in &s.presets {
        list = list.child(if s.preset_deleting.as_deref() == Some(p.name.as_str()) { confirm_row(p) } else { preset_row(p, p.look == look) });
    }
    let has_look = look != darkroom_core::Edit::default();
    let head = row().w_full().h(36.0).px(12.0).center_y().child(text("Presets").font(t::HEADING).color(t::INK));
    let body = col().w_full().px(12.0).pb(16.0).gap(12.0).child(list).child(if s.naming_preset { name_field(s) } else { save_button(has_look) });
    col().key("presets").w_full().child(head).child(body).child(rule())
}

/// A preset: its name applies it, the cross beside it asks to delete it.
fn preset_row(p: &Preset, current: bool) -> El {
    let (apply, delete) = (p.name.clone(), p.name.clone());
    let name = row()
        .key(format!("preset-{}", p.name))
        .role("button")
        .label(format!("Apply {}", p.name))
        .checked(current)
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h(28.0)
        .px(8.0)
        .center_y()
        .radius(7.0)
        .fill(if current { t::BG_ACTIVE } else { Color::TRANSPARENT })
        .hover(if current { t::BG_ACTIVE } else { t::BG_HOVER })
        .on_press(move |s: &mut State| s.apply_preset(&apply))
        .child(text(p.name.clone()).font(if current { t::BODY_STRONG } else { t::BODY }).color(t::INK).truncate_fade().shrink());
    let cross = el()
        .key(format!("preset-delete-{}", p.name))
        .role("button")
        .label(format!("Delete {}", p.name))
        .size(24.0, 24.0)
        .center()
        .radius(7.0)
        .hover(t::BG_HOVER)
        .on_press(move |s: &mut State| s.preset_deleting = Some(delete.clone()))
        .child(canvas(cross_icon(t::INK_FAINT)).size(16.0, 16.0));
    row().w_full().gap(4.0).center_y().child(name).child(cross)
}

/// A preset about to be deleted: the question in its place, until Delete or Cancel.
fn confirm_row(p: &Preset) -> El {
    let name = p.name.clone();
    row()
        .w_full()
        .h(28.0)
        .pl(8.0)
        .gap(8.0)
        .center_y()
        .child(text(format!("Delete “{}”?", p.name)).font(t::LABEL).color(t::INK).truncate_fade().shrink())
        .child(spacer())
        .child(button("preset-delete-cancel", "Cancel", |s| s.preset_deleting = None))
        .child(button("preset-delete-confirm", "Delete", move |s| s.delete_preset(&name)))
}

fn save_button(has_look: bool) -> El {
    let b = button("preset-save", "Save preset…", State::start_preset).self_start();
    // An unedited look would be a preset that does nothing.
    if has_look { b } else { b.disabled(true).dim(0.45) }
}

/// The new preset's name, typed in place of the Save button: Return or Save keeps it, Esc
/// drops it. A name that can't be used says why under the field.
fn name_field(s: &State) -> El {
    let input = text_input(&s.preset_name, t::BODY)
        .single_line()
        .key("preset-name")
        .label("Preset name")
        .placeholder("Name", t::INK_FAINT)
        .grow(1.0)
        .color(t::INK)
        .selection_color(t::ACCENT_SOFT)
        .on_edit(|s: &mut State| &mut s.preset_name)
        .on_submit(|s: &mut State, tx: &mut Tx| {
            s.save_preset();
            if !s.naming_preset {
                tx.blur();
            }
        })
        .on_input_key(|s: &mut State, ev: &KeyEvent, tx: &mut Tx| {
            if ev.key != Key::Escape {
                return false;
            }
            s.naming_preset = false;
            tx.blur();
            true
        });
    let typed = s.preset_name.text.trim();
    let problem = if typed.is_empty() { None } else { preset::check_name(typed, &s.presets).err() };
    let field = row().h(28.0).grow(1.0).shrink().min_w(0.0).px(8.0).radius(7.0).center_y().fill(t::BG_RAISED).border(1.0, t::ACCENT).outline(3.0, 0.0, t::ACCENT_SOFT).child(input);
    let save = button("preset-save-confirm", "Save", State::save_preset);
    let save = if typed.is_empty() || problem.is_some() { save.disabled(true).dim(0.45) } else { save };
    let entry = row().w_full().gap(8.0).center_y().child(field).child(save);
    col().w_full().gap(6.0).child(entry).children(problem.map(|e| text(e.to_string()).font(t::CAPTION).color(t::DANGER_INK).wrap()))
}
