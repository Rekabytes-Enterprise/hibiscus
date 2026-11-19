use super::*;
fn text(row: &Row) -> String {
    row.cells.iter().map(|cell| cell.ch).collect()
}

#[test]
fn layout_matches_full_format_across_streaming_resize_and_replacement() {
    let mut layout = Layout::default();
    let text = "you › literal **text**\n  · ◐ Exploring · file\n  · ✓ Read · 1 file\nhibi › ## Title\n| A | B |\n| --- | --- |\n```rust\nlet x = 1;\n```\n\nyou › next\nhibi › [link](url)";
    for width in [73, 12, 1, 73] {
        for end in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            layout.update(&text[..end], width);
            assert_eq!(
                layout
                    .rows()
                    .iter()
                    .map(|r| (**r).clone())
                    .collect::<Vec<_>>(),
                format(&text[..end], width)
            );
        }
    }
    let first = layout.rows()[0].clone();
    layout.update(&format!("{text} more"), 73);
    assert!(std::rc::Rc::ptr_eq(&first, &layout.rows()[0]));
    layout.update("hibi › replacement", 73);
    assert_eq!(
        layout
            .rows()
            .iter()
            .map(|r| (**r).clone())
            .collect::<Vec<_>>(),
        format("hibi › replacement", 73)
    );
}

#[test]
fn long_lines_and_unmatched_links_remain_literal() {
    for source in ["[".repeat(65536), "x".repeat(65536), " ".repeat(65536)] {
        let rows = format(&source, 73);
        assert!(rows.iter().all(|r| r.cells.len() <= 73));
        if !source.starts_with(' ') {
            assert_eq!(
                rows.iter()
                    .map(|r| text(r).trim_start().to_owned())
                    .collect::<String>(),
                source
            );
        }
    }
    assert_eq!(text(&format("[no closing](", 73)[0]), "[no closing](");
}

#[test]
fn tool_first_turns_share_a_speaker_with_prose_and_reset_for_the_next_user() {
    for activity in [
        "◐ Exploring",
        "◓ Exploring",
        "◑ Exploring",
        "◒ Exploring",
        "Explore stopped",
        "✓ Explored",
        "✓ Read",
        "✓ Bash",
        "✗ Bash",
        "◌ edit",
    ] {
        let input = format!("you › First\n  · {activity} · file\nhibi › Done.\nyou › Second\n  · ✓ Bash · 1 command");
        let rows = format(&input, 80);
        let lines: Vec<_> = rows.iter().map(text).collect();
        assert_eq!(lines.iter().filter(|line| *line == "hibi").count(), 2);
        assert_eq!(lines[1], "hibi", "{activity}");
        assert_eq!(lines[5], "hibi", "{activity}");
    }
    let rows = format("  · Login cancelled.\nhibi › \n\n", 80);
    assert!(!rows.iter().any(|row| text(row) == "hibi"));
    let rows = format(
        "you › First\nhibi › Reading.\n  · ✓ Read · 1 file\nhibi › Done.",
        80,
    );
    assert_eq!(rows.iter().filter(|row| text(row) == "hibi").count(), 1);
}

#[test]
fn renders_lists_bold_code_links_and_fences_without_markers() {
    let rows = format("hibiscus › ## Overview\n- **Bold** and `code` [link](https://example.com)\n```rust\nlet x = 2;\n```", 100);
    assert_eq!(
        rows.iter().map(text).collect::<Vec<_>>(),
        [
            "hibi",
            "Overview",
            "• Bold and code link",
            "  rust",
            "  let x = 2;",
            ""
        ]
    );
    assert!(rows[1].cells.iter().all(|cell| cell.tone == Tone::Heading));
    assert_eq!(rows[2].cells[2].tone, Tone::Strong);
    assert!(rows[4]
        .cells
        .iter()
        .skip(2)
        .all(|cell| cell.tone == Tone::Code));
}

#[test]
fn wraps_without_losing_roles_and_keeps_user_text_literal() {
    let rows = format("you › **literal**\nhibiscus › **hello** world", 7);
    assert_eq!(text(&rows[0]), "**liter");
    assert_eq!(rows[0].role, Role::User);
    assert_eq!(rows.last().unwrap().role, Role::Assistant);
    assert_eq!(
        rows.iter()
            .filter(|row| row.role == Role::User)
            .map(|row| text(row).trim_start().to_owned())
            .collect::<Vec<_>>()
            .join(""),
        "**literal**"
    );
}

#[test]
fn wraps_at_words_and_indents_continuations() {
    let rows = format("hibiscus › - testing a fairly long line", 15);
    assert_eq!(
        rows.iter().map(text).collect::<Vec<_>>(),
        ["hibi", "• testing a", "  fairly long", "  line"]
    );
    assert!(rows.iter().all(|r| r.cells.len() <= 15));
}

#[test]
fn unmatched_markers_remain_visible() {
    let rows = format("hibiscus › src/my_file_name.rs and **unfinished", 80);
    assert_eq!(text(&rows[1]), "src/my_file_name.rs and **unfinished");
}

#[test]
fn unclosed_fence_does_not_leak_into_next_reply() {
    let rows = format(
        "hibiscus › ```rust\nlet x = 1\nyou › next\nhibiscus › **answer**",
        80,
    );
    assert_eq!(text(rows.last().unwrap()), "answer");
    assert_eq!(rows.last().unwrap().cells[0].tone, Tone::Strong);
}

#[test]
fn colors_activity_and_keeps_edit_diff_literal() {
    let rows = format("  · reasoning · 3s\n  · diff · updated.md\n     11 context\n    -12 **old**\n    +12 `new`\n  · ✓ done", 80);
    assert_eq!(rows[0].cells[0].tone, Tone::Activity);
    assert_eq!(rows[1].cells[0].tone, Tone::Activity);
    assert_eq!(rows[2].role, Role::Diff);
    assert_eq!(rows[2].cells[0].tone, Tone::DiffContext);
    assert_eq!(text(&rows[3]), "-12 **old**");
    assert_eq!(rows[3].cells[0].tone, Tone::DiffRemoved);
    assert_eq!(text(&rows[4]), "+12 `new`");
    assert_eq!(rows[4].cells[0].tone, Tone::DiffAdded);
    assert_eq!(rows[5].cells[0].tone, Tone::Success);
}

#[test]
fn mermaid_stays_source_until_a_completed_run_supplies_art() {
    let message = "you › ```mermaid\nflowchart LR\n  X --> Y\n```\n\nhibi › Before\n```mermaid\nflowchart LR\n  A[Start] --> B[Done]\n```\nAfter";
    let body = "flowchart LR\n  A[Start] --> B[Done]\n";
    assert_eq!(mermaid_sources(message), vec![body]);
    let mut layout = Layout::default();
    layout.update(message, 60);
    let raw: Vec<String> = layout.rows().iter().map(|row| text(row)).collect();
    assert!(raw.iter().any(|line| line.contains("A[Start] --> B[Done]")));
    let mut diagrams = HashMap::new();
    diagrams.insert(
        body.into(),
        vec!["╭─────╮".into(), "│ Done│".into(), "╰─────╯".into()],
    );
    layout.invalidate();
    layout.update_with_diagrams(message, 60, &diagrams);
    let final_rows: Vec<String> = layout.rows().iter().map(|row| text(row)).collect();
    assert!(final_rows.iter().any(|line| line == "Before"));
    assert!(final_rows.iter().any(|line| line == "After"));
    assert!(final_rows.iter().any(|line| line == "╭─────╮"));
    assert!(!final_rows
        .iter()
        .any(|line| line.contains("A[Start] --> B[Done]")));
    layout.update_with_diagrams(message, 5, &diagrams);
    assert!(layout.rows().iter().any(|row| text(row).contains("flow")));
    // A partial fence never consumes the chat that follows it.
    assert!(mermaid_sources("hibi › ```mermaid\nA --> B").is_empty());
    assert!(mermaid_sources("hibi › ```mermaid\nA --> B\nyou › ```\nhibi › Later").is_empty());
    assert!(mermaid_sources(
        "hibi › prose\n  · diff · file\n```mermaid\nflowchart LR\n  A --> B\n```"
    )
    .is_empty());
}

#[test]
fn formats_tables_and_numbered_lists() {
    let rows = format(
        "hibiscus › | Name | Value |\n| :--- | ---: |\n| pink | 2 |\n1. first\n---",
        80,
    );
    assert_eq!(
        rows.iter().map(text).collect::<Vec<_>>(),
        [
            "hibi",
            "╭──────┬───────╮",
            "│ Name │ Value │",
            "├──────┼───────┤",
            "│ pink │ 2     │",
            "╰──────┴───────╯",
            "1. first",
            "────────"
        ]
    );
    assert_eq!(rows[2].cells[2].tone, Tone::Strong);
}

#[test]
fn formats_tables_without_outer_pipes() {
    let rows = format(
        "hibiscus › Name | Value\n:--- | ---:\npink | 2\nplain | text\n\nnot | a table",
        80,
    );
    assert_eq!(
        rows.iter().map(text).collect::<Vec<_>>(),
        [
            "hibi",
            "╭───────┬───────╮",
            "│ Name  │ Value │",
            "├───────┼───────┤",
            "│ pink  │ 2     │",
            "├───────┼───────┤",
            "│ plain │ text  │",
            "╰───────┴───────╯",
            "",
            "not | a table"
        ]
    );
    assert_eq!(rows[2].cells[2].tone, Tone::Strong);
    assert_eq!(rows[4].cells[2].tone, Tone::Plain);
}

#[test]
fn wraps_long_table_cells_without_shifting_columns() {
    let rows = format(
        "hibi › Aspect | JavaScript | TypeScript\nTypes | Dynamic; errors appear at runtime | Static types catch many errors before runtime\nRuntime | Runs directly in browsers and Node.js | Usually compiled to JavaScript first",
        55,
    );
    let lines: Vec<String> = rows.iter().map(text).collect();
    let table: Vec<&String> = lines.iter().filter(|line| line.starts_with('│')).collect();
    assert!(table.len() > 3, "{lines:?}");
    assert!(rows.iter().all(|row| row.cells.len() <= 55), "{lines:?}");
    let borders: Vec<usize> = table[0].match_indices('│').map(|(i, _)| i).collect();
    for line in table {
        assert_eq!(
            line.match_indices('│').map(|(i, _)| i).collect::<Vec<_>>(),
            borders,
            "{line}"
        );
    }
    assert!(lines.iter().any(|line| line.contains("errors appear")));
    assert!(lines.iter().any(|line| line.contains("JavaScript first")));
}

#[test]
fn formats_consecutive_pipe_rows_without_a_separator() {
    let rows = format(
        "hibiscus › Aspect | JavaScript | TypeScript\nTypes | Dynamic | Static\nRuntime | Direct | Compiled\n\nA | single row stays literal",
        100,
    );
    assert_eq!(
        rows.iter().map(text).collect::<Vec<_>>(),
        [
            "hibi",
            "╭─────────┬────────────┬────────────╮",
            "│ Aspect  │ JavaScript │ TypeScript │",
            "├─────────┼────────────┼────────────┤",
            "│ Types   │ Dynamic    │ Static     │",
            "├─────────┼────────────┼────────────┤",
            "│ Runtime │ Direct     │ Compiled   │",
            "╰─────────┴────────────┴────────────╯",
            "",
            "A | single row stays literal"
        ]
    );
    assert_eq!(rows[2].cells[2].tone, Tone::Strong);
    assert_eq!(rows[4].cells[2].tone, Tone::Plain);
}
