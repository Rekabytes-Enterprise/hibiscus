/// Provider-style tabs for a searchable picker: tab names, each item's tab
/// index, the prefix shown on the "All" tab, and the starting tab.
pub(crate) struct Tabs {
    pub(crate) names: Vec<String>,
    pub(crate) groups: Vec<usize>,
    pub(crate) prefixes: Vec<String>,
    pub(crate) tab: usize,
    /// Initial search text, e.g. from `/models openrouter claude`.
    pub(crate) query: String,
}

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
    /// Tab labels and each source item's tab; the last tab shows every item.
    pub(crate) tabs: Vec<String>,
    pub(crate) tab: usize,
    groups: Vec<usize>,
    /// Shown before an item's label on the all-items tab, e.g. its provider.
    prefixes: Vec<String>,
    max_rows: usize,
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
            tabs: Vec::new(),
            tab: 0,
            groups: Vec::new(),
            prefixes: Vec::new(),
            max_rows: 5,
        };
        picker.ensure_visible(rows);
        picker
    }

    pub(crate) fn enable_search(&mut self) {
        self.source = self.items.clone();
        self.indices = (0..self.items.len()).collect();
        self.query = Some(String::new());
    }

    /// Groups searchable items into tabs plus a final "All" tab, starting on `tab`.
    pub(crate) fn enable_tabs(
        &mut self,
        names: &[String],
        groups: Vec<usize>,
        prefixes: Vec<String>,
        tab: usize,
        rows: usize,
    ) {
        self.tabs = names
            .iter()
            .enumerate()
            .map(|(i, name)| format!("{name} {}", groups.iter().filter(|&&g| g == i).count()))
            .chain([format!("All {}", groups.len())])
            .collect();
        self.groups = groups;
        self.prefixes = prefixes;
        self.tab = tab.min(self.tabs.len() - 1);
        self.max_rows = 10;
        self.refilter(self.source_current, rows);
    }

    pub(crate) fn switch_tab(&mut self, step: isize, rows: usize) {
        if self.tabs.is_empty() {
            return;
        }
        let count = self.tabs.len() as isize;
        self.tab = (self.tab as isize + step).rem_euclid(count) as usize;
        self.refilter(self.source_current, rows);
    }

    /// Rows used by the panel around its choices.
    pub(crate) fn chrome(&self) -> usize {
        3 + 3 * usize::from(self.query.is_some()) + usize::from(!self.tabs.is_empty())
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
        self.refilter(previous, rows);
    }

    fn label(&self, index: usize) -> String {
        match self.prefixes.get(index) {
            Some(prefix) if self.tab + 1 == self.tabs.len() => {
                format!("{prefix}{}", self.source[index])
            }
            _ => self.source[index].clone(),
        }
    }

    fn refilter(&mut self, previous: Option<usize>, rows: usize) {
        let terms = self.query.as_deref().unwrap_or("").to_lowercase();
        let all = self.tab + 1 >= self.tabs.len();
        self.indices = (0..self.source.len())
            .filter(|&index| all || self.groups.get(index) == Some(&self.tab))
            .filter(|&index| {
                // Search the provider too, so "openrouter claude" works on All.
                let label = format!(
                    "{}{}",
                    self.prefixes.get(index).map_or("", String::as_str),
                    self.source[index]
                )
                .to_lowercase();
                terms.split_whitespace().all(|term| label.contains(term))
            })
            .collect();
        self.items = self
            .indices
            .iter()
            .map(|&index| self.label(index))
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
        self.items.len().min(rows.clamp(1, self.max_rows))
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
    fn tabs_filter_by_group_and_all_tab_shows_prefixes_and_searches_them() {
        let mut picker = Picker::new(
            "Models".into(),
            vec!["a".into(), "claude".into(), "claude".into()],
            Some(2),
            10,
        );
        picker.enable_search();
        picker.enable_tabs(
            &["one".into(), "two".into()],
            vec![0, 0, 1],
            vec!["one/".into(), "one/".into(), "two/".into()],
            1,
            10,
        );
        assert_eq!(picker.tabs, ["one 2", "two 1", "All 3"]);
        assert_eq!(
            (picker.items.clone(), picker.current),
            (vec!["claude".to_owned()], Some(0))
        );
        assert_eq!(picker.chrome(), 7);
        picker.switch_tab(1, 10);
        assert_eq!(picker.items, ["one/a", "one/claude", "two/claude"]);
        assert_eq!(picker.selected_index(), Some(2));
        picker.search("two claude", false, 10);
        assert_eq!(picker.items, ["two/claude"]);
        picker.switch_tab(1, 10);
        assert_eq!(picker.tab, 0);
        assert!(picker.items.is_empty());
        picker.switch_tab(-1, 10);
        assert_eq!(picker.tab, 2);
        let mut long = Picker::new(
            "Models".into(),
            (0..30).map(|n| n.to_string()).collect(),
            None,
            40,
        );
        long.enable_search();
        long.enable_tabs(&["x".into()], vec![0; 30], vec!["x/".into(); 30], 0, 40);
        assert_eq!(long.visible(40), 10);
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
