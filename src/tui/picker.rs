/// Shared selection/viewport state for in-screen pickers. The renderer lives in
/// Screen, while models and saved sessions provide their own labels and IDs.
pub(crate) struct Picker {
    pub(crate) title: String,
    pub(crate) items: Vec<String>,
    pub(crate) selected: usize,
    pub(crate) current: Option<usize>,
    pub(crate) first: usize,
}

impl Picker {
    pub(crate) fn new(
        title: String,
        items: Vec<String>,
        current: Option<usize>,
        rows: usize,
    ) -> Self {
        let selected = current.filter(|&index| index < items.len()).unwrap_or(0);
        let mut picker = Self {
            title,
            items,
            selected,
            current,
            first: 0,
        };
        picker.ensure_visible(rows);
        picker
    }

    pub(crate) fn visible(&self, rows: usize) -> usize {
        self.items.len().min(rows.clamp(1, 5))
    }

    fn ensure_visible(&mut self, rows: usize) {
        let visible = self.visible(rows);
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + visible {
            self.first = self.selected + 1 - visible;
        }
        self.first = self.first.min(self.items.len().saturating_sub(visible));
    }

    pub(crate) fn move_by(&mut self, step: isize, rows: usize) {
        if self.items.is_empty() {
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(step)
            .min(self.items.len() - 1);
        self.ensure_visible(rows);
    }

    pub(crate) fn page(&mut self, step: isize, rows: usize) {
        self.move_by(
            step.saturating_mul(self.visible(rows).max(1) as isize),
            rows,
        );
    }

    pub(crate) fn thumb(&self, rows: usize) -> usize {
        let visible = self.visible(rows);
        if self.items.len() <= visible {
            return 0;
        }
        self.first * (visible - 1) / (self.items.len() - visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_follows_arrows_and_keeps_five_visible() {
        let mut picker = Picker::new(
            "Models".into(),
            (0..12).map(|n| n.to_string()).collect(),
            Some(7),
            5,
        );
        assert_eq!(
            (picker.selected, picker.first, picker.visible(5)),
            (7, 3, 5)
        );
        picker.move_by(1, 5);
        assert_eq!((picker.selected, picker.first), (8, 4));
        picker.page(1, 5);
        assert_eq!((picker.selected, picker.first, picker.thumb(5)), (11, 7, 4));
        picker.move_by(-30, 5);
        assert_eq!((picker.selected, picker.first), (0, 0));
    }

    #[test]
    fn empty_or_short_list_cannot_overflow() {
        let mut picker = Picker::new("empty".into(), vec![], Some(12), 5);
        picker.move_by(1, 5);
        assert_eq!(picker.visible(5), 0);
        let mut picker = Picker::new("one".into(), vec!["a".into()], Some(99), 5);
        picker.move_by(1, 5);
        assert_eq!((picker.selected, picker.first, picker.thumb(5)), (0, 0, 0));
    }
}
