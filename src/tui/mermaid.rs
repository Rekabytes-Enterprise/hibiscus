//! Bounded native rendering for simple linear top-to-bottom flowcharts.
//! Unsupported graphs remain Mermaid source rather than being misrepresented.
use std::collections::{HashMap, HashSet};
pub(crate) fn render(sources: &[String]) -> HashMap<String, Vec<String>> {
    if sources.len() > 8 || sources.iter().map(String::len).sum::<usize>() > 8192 {
        return HashMap::new();
    }
    sources
        .iter()
        .filter_map(|s| linear(s).map(|rows| (s.clone(), rows)))
        .collect()
}
fn node(source: &str) -> Option<(String, String)> {
    let source = source.trim().trim_end_matches(';');
    let (id, label) = if let Some((id, label)) = source.split_once('[') {
        (id, label.strip_suffix(']')?.trim_matches('"'))
    } else {
        (source, source)
    };
    if id.is_empty()
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || label.chars().count() > 60
        || label.chars().any(char::is_control)
    {
        return None;
    }
    Some((id.into(), label.into()))
}
fn linear(source: &str) -> Option<Vec<String>> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("%%"));
    if !matches!(
        lines.next()?,
        "flowchart TD" | "flowchart TB" | "graph TD" | "graph TB"
    ) {
        return None;
    }
    let mut names = HashMap::new();
    let mut next = HashMap::new();
    let mut incoming = HashSet::new();
    for line in lines {
        let parts: Vec<_> = line.split("-->").collect();
        if parts.len() < 2 {
            return None;
        }
        for pair in parts.windows(2) {
            let (a, la) = node(pair[0])?;
            let (b, lb) = node(pair[1])?;
            if next.insert(a.clone(), b.clone()).is_some() || !incoming.insert(b.clone()) {
                return None;
            }
            names.entry(a).or_insert(la);
            names.entry(b).or_insert(lb);
        }
    }
    let roots = names
        .keys()
        .filter(|k| !incoming.contains(*k))
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return None;
    }
    let mut current = roots[0].clone();
    let mut visited = HashSet::new();
    let mut ordered = vec![];
    loop {
        if !visited.insert(current.clone()) {
            return None;
        }
        ordered.push(names.get(&current)?.clone());
        match next.get(&current) {
            Some(n) => current = n.clone(),
            None => break,
        }
    }
    if visited.len() != names.len() {
        return None;
    }
    let width = ordered.iter().map(|s| s.chars().count()).max()?;
    let border = "─".repeat(width + 2);
    let mut rows = vec![];
    for (i, label) in ordered.iter().enumerate() {
        if i > 0 {
            rows.push(format!("{}│", " ".repeat((width + 4) / 2)));
            rows.push(format!("{}▼", " ".repeat((width + 4) / 2)));
        }
        rows.push(format!("┌{border}┐"));
        rows.push(format!(
            "│ {label}{} │",
            " ".repeat(width - label.chars().count())
        ));
        rows.push(format!("└{border}┘"));
    }
    Some(rows)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_only_complete_linear_graphs() {
        let text = "flowchart TD\nA[\"Read\"] --> B[\"Edit\"] --> C[\"Test\"]";
        let rows = linear(text).unwrap();
        assert!(rows.iter().any(|r| r.contains("Read")));
        assert!(rows.iter().any(|r| r.contains("Test")));
        assert!(linear("flowchart TD\nA --> B\nA --> C").is_none());
        assert!(linear("flowchart TD\nA --> B\nB --> A").is_none());
        assert!(linear("sequenceDiagram\nA -> B").is_none());
    }
}
