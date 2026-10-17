/// Shared selection/viewport state for in-screen pickers. The renderer lives in
/// Screen, while models and saved sessions provide their own labels and IDs.
pub(crate) struct Picker {
    pub(crate) title: String,
    pub(crate) items: Vec<String>,
    pub(crate) selected: usize,
    pub(crate) current: Option<usize>,
    pub(crate) first: usize,
    pub(crate) query: Option<String>,
    source: Vec<String>,
    indices: Vec<usize>,
    source_current: Option<usize>,
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
            query: None,
            source: Vec::new(),
            indices: Vec::new(),
            source_current: current,
        };
        picker.ensure_visible(rows);
        picker
    }

    pub(crate) fn enable_search(&mut self) {
        self.source = self.items.clone();
        self.indices = (0..self.items.len()).collect();
        self.query = Some(String::new());
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.items.get(self.selected)?;
        if self.query.is_some() {
            self.indices.get(self.selected).copied()
        } else {
            Some(self.selected)
        }
    }

    pub(crate) fn search(&mut self, text: &str, backspace: bool, rows: usize) {
        let previous = self.selected_index();
        let Some(query) = &mut self.query else { return };
        if backspace {
            query.pop();
        } else {
            for ch in text.chars().filter(|ch| !ch.is_control()) {
                if query.len() + ch.len_utf8() <= 256 {
                    query.push(ch);
                }
            }
        }
        let terms = query.to_lowercase();
        self.indices = self
            .source
            .iter()
            .enumerate()
            .filter_map(|(index, label)| {
                let label = label.to_lowercase();
                terms
                    .split_whitespace()
                    .all(|term| label.contains(term))
                    .then_some(index)
            })
            .collect();
        self.items = self
            .indices
            .iter()
            .map(|&index| self.source[index].clone())
            .collect();
        self.current = self
            .source_current
            .and_then(|index| self.indices.iter().position(|&i| i == index));
        self.selected = previous
            .and_then(|index| self.indices.iter().position(|&i| i == index))
            .unwrap_or(0);
        self.first = 0;
        self.ensure_visible(rows);
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
    fn search_maps_results_to_original_models_and_handles_empty_results() {
        let mut picker = Picker::new(
            "Models".into(),
            vec![
                "demo/old".into(),
                "codex/GPT".into(),
                "other/GPT".into(),
                "demo/é".into(),
            ],
            Some(1),
            5,
        );
        picker.enable_search();
        picker.search("GPT CODEX", false, 5);
        assert_eq!(picker.items, ["codex/GPT"]);
        assert_eq!(picker.current, Some(0));
        assert_eq!(picker.selected_index(), Some(1));
        picker.search("zzz", false, 5);
        assert!(picker.items.is_empty());
        assert_eq!(picker.selected_index(), None);
        picker.move_by(1, 5);
        for _ in 0..12 {
            picker.search("", true, 5);
        }
        assert_eq!(picker.items.len(), 4);
        picker.search("é", false, 5);
        assert_eq!(picker.selected_index(), Some(3));
        picker.search("", true, 5);
        assert_eq!(picker.query.as_deref(), Some(""));
        assert_eq!(picker.items.len(), 4);
    }

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
