//! Undo and redo for one photo's adjustments.
//!
//! A [`History`] is a list of whole edits, one per finished change, and a position in it. Undo
//! and redo move the position; a new change drops everything after it. Rating and flag aren't
//! adjustments: steps don't hold them, and undo leaves them as they are.

use crate::{Edit, Flag};

/// Steps kept per photo; past this the oldest is forgotten.
const LIMIT: usize = 200;

#[derive(Clone, Debug)]
pub struct History {
    /// Every recorded state, oldest first, with rating and flag cleared. Never empty.
    steps: Vec<Edit>,
    /// The step the photo is at.
    at: usize,
}

impl History {
    /// A history that starts at `edit`, with nothing to undo.
    pub fn new(edit: &Edit) -> History {
        History { steps: vec![adjustments(edit)], at: 0 }
    }

    /// Whether `edit`'s adjustments are the ones this history is at.
    pub fn is_at(&self, edit: &Edit) -> bool {
        self.steps[self.at] == adjustments(edit)
    }

    /// Records `edit` as a new step and drops anything undone. An edit whose adjustments
    /// haven't changed records nothing.
    pub fn record(&mut self, edit: &Edit) {
        if self.is_at(edit) {
            return;
        }
        self.steps.truncate(self.at + 1);
        self.steps.push(adjustments(edit));
        if self.steps.len() > LIMIT {
            self.steps.remove(0);
        }
        self.at = self.steps.len() - 1;
    }

    pub fn can_undo(&self) -> bool {
        self.at > 0
    }

    pub fn can_redo(&self) -> bool {
        self.at + 1 < self.steps.len()
    }

    /// Steps back: `current` with the previous step's adjustments, or None at the start.
    pub fn undo(&mut self, current: &Edit) -> Option<Edit> {
        if !self.can_undo() {
            return None;
        }
        self.at -= 1;
        Some(self.applied(current))
    }

    /// Steps forward again: `current` with the next step's adjustments, or None at the end.
    pub fn redo(&mut self, current: &Edit) -> Option<Edit> {
        if !self.can_redo() {
            return None;
        }
        self.at += 1;
        Some(self.applied(current))
    }

    fn applied(&self, current: &Edit) -> Edit {
        Edit { rating: current.rating, flag: current.flag, ..self.steps[self.at].clone() }
    }
}

/// `edit` without its rating and flag.
fn adjustments(edit: &Edit) -> Edit {
    Edit { rating: 0, flag: Flag::None, ..edit.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposed(stops: f32) -> Edit {
        let mut e = Edit::default();
        e.light.exposure = stops;
        e
    }

    #[test]
    fn a_new_history_has_nothing_to_undo_or_redo() {
        let mut h = History::new(&exposed(1.0));
        assert!(!h.can_undo() && !h.can_redo());
        assert_eq!(h.undo(&exposed(1.0)), None);
        assert_eq!(h.redo(&exposed(1.0)), None);
    }

    #[test]
    fn undo_walks_back_and_redo_walks_forward() {
        let mut h = History::new(&Edit::default());
        h.record(&exposed(1.0));
        h.record(&exposed(2.0));
        assert_eq!(h.undo(&exposed(2.0)), Some(exposed(1.0)));
        assert_eq!(h.undo(&exposed(1.0)), Some(Edit::default()));
        assert!(!h.can_undo());
        assert_eq!(h.redo(&Edit::default()), Some(exposed(1.0)));
        assert_eq!(h.redo(&exposed(1.0)), Some(exposed(2.0)));
        assert!(!h.can_redo());
    }

    #[test]
    fn an_unchanged_edit_records_no_step() {
        let mut h = History::new(&exposed(1.0));
        h.record(&exposed(1.0));
        assert!(!h.can_undo());
    }

    #[test]
    fn a_new_change_drops_what_was_undone() {
        let mut h = History::new(&Edit::default());
        h.record(&exposed(1.0));
        h.undo(&exposed(1.0));
        h.record(&exposed(2.0));
        assert!(!h.can_redo());
        assert_eq!(h.undo(&exposed(2.0)), Some(Edit::default()));
    }

    #[test]
    fn rating_and_flag_are_not_steps_and_survive_undo() {
        let mut h = History::new(&Edit::default());
        h.record(&exposed(1.0));
        let rated = Edit { rating: 4, flag: Flag::Pick, ..exposed(1.0) };
        h.record(&rated);
        assert!(h.is_at(&rated));
        assert_eq!(h.undo(&rated), Some(Edit { rating: 4, flag: Flag::Pick, ..Edit::default() }));
        assert!(!h.can_undo());
    }

    #[test]
    fn the_oldest_steps_are_forgotten_past_the_limit() {
        let mut h = History::new(&Edit::default());
        for i in 1..=LIMIT + 10 {
            h.record(&exposed(i as f32));
        }
        let mut undone = 0;
        let mut last = None;
        while let Some(e) = h.undo(&Edit::default()) {
            undone += 1;
            last = Some(e);
        }
        assert_eq!(undone, LIMIT - 1);
        assert_eq!(last, Some(exposed(11.0)));
    }
}
