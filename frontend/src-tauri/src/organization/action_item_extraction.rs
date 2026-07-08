/// Extracts structured action items from a generated summary's markdown,
/// so Phase 5's cross-note action item view (organization::action_items)
/// actually has something to show. Every built-in template asks the model
/// for an action-item-shaped section, but until this module existed nothing
/// ever turned that markdown text into action_items rows.
///
/// Templates render this content two ways -- see frontend/src-tauri/templates/*.json:
///   - as a markdown table (standard_meeting, project_sync, retrospective,
///     sales_marketing_client_call): "| **Owner** | Task | Due | ... |"
///   - as bullet items (lecture, discussion): "- **Assignment**: text — *Due: date*"
/// Both are handled. Per PROJECT_BRIEF.md §12 ("a fabricated due date is a
/// worse failure than a missing one"), fields are only ever populated from
/// an explicit match -- ambiguous text leaves owner/due_date as None rather
/// than guessing, and template-instructed placeholder text ("none
/// mentioned", "unassigned", "no date mentioned") is recognized and dropped
/// rather than stored as a literal fake item/value.
use crate::markdown_blocks::{parse_markdown_blocks, Block};

/// Section headings that mean "this section's items are personal action
/// items" as opposed to project-management content like "Milestones" or
/// "Risks" that happens to also use a table -- those have fuzzier
/// owner/task semantics and are deliberately not auto-extracted.
const ACTION_SECTION_KEYWORDS: &[&str] = &["action item", "todo", "to-do", "homework", "assignment"];

const PLACEHOLDER_VALUES: &[&str] = &[
    "none",
    "none mentioned",
    "n/a",
    "na",
    "nothing",
    "nothing mentioned",
    "unassigned",
    "no date mentioned",
    "not mentioned",
    "not specified",
    "unspecified",
    "tbd",
    "-",
    "—",
];

fn is_placeholder(text: &str) -> bool {
    let normalized = text.trim().trim_matches(|c: char| c == '*' || c == '.').to_lowercase();
    normalized.is_empty() || PLACEHOLDER_VALUES.contains(&normalized.as_str())
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedActionItem {
    pub description: String,
    pub owner: Option<String>,
    pub due_date: Option<String>,
}

fn is_action_heading(text: &str) -> bool {
    let lower = text.to_lowercase();
    ACTION_SECTION_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// Strip a leading "Label: " prefix like the "**Assignment**: " in the
/// lecture template's item_format, once markdown emphasis has already been
/// flattened to plain text by parse_markdown_blocks.
fn strip_leading_label(text: &str) -> &str {
    if let Some(idx) = text.find(": ") {
        let label = &text[..idx];
        let looks_like_label = !label.is_empty()
            && label.split_whitespace().count() <= 3
            && label.chars().next().is_some_and(|c| c.is_uppercase());
        if looks_like_label {
            return text[idx + 2..].trim();
        }
    }
    text
}

fn looks_like_date(text: &str) -> bool {
    let t = text.trim();
    // ISO-ish (2026-08-01) or has a digit alongside a month name / weekday --
    // deliberately loose since transcripts produce dates in many shapes.
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    let has_month_or_day = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    ]
    .iter()
    .any(|m| t.to_lowercase().contains(m));
    has_digit || has_month_or_day
}

/// Parse a bullet-item's flattened text into an item. Handles both
/// "- **Assignment**: text — *Due: date*" (lecture) and
/// "- task — owner — due date" (discussion) shapes, which both use an
/// em-dash as the field separator.
fn parse_bullet_item(text: &str) -> Option<ExtractedActionItem> {
    let text = text.trim();
    if is_placeholder(text) {
        return None;
    }

    let parts: Vec<&str> = text.split('—').map(|s| s.trim()).collect();
    let description = strip_leading_label(parts[0]).trim();
    if description.is_empty() || is_placeholder(description) {
        return None;
    }

    let mut owner = None;
    let mut due_date = None;

    for part in &parts[1..] {
        let lower = part.to_lowercase();
        if let Some(idx) = lower.find("due:") {
            // ASCII-length-preserving lowercase, so slicing the original
            // string at the same byte offset the lowercase match found is safe
            let date = part[idx + "due:".len()..].trim();
            if !is_placeholder(date) {
                due_date = Some(date.to_string());
            }
        } else if is_placeholder(part) {
            // explicit "unassigned" / "no date mentioned" -- skip silently
        } else if looks_like_date(part) && due_date.is_none() {
            due_date = Some(part.to_string());
        } else if owner.is_none() {
            owner = Some(part.to_string());
        }
    }

    Some(ExtractedActionItem { description: description.to_string(), owner, due_date })
}

/// Parse one table row into an item using header keywords to find the
/// owner/due-date/task columns; falls back to the first non-owner,
/// non-due column as the description when no column looks task-shaped.
fn parse_table_row(headers: &[String], row: &[String]) -> Option<ExtractedActionItem> {
    let mut description = None;
    let mut owner = None;
    let mut due_date = None;

    for (header, cell) in headers.iter().zip(row.iter()) {
        let h = header.to_lowercase();
        let cell = cell.trim();
        if cell.is_empty() || is_placeholder(cell) {
            continue;
        }
        if h.contains("owner") {
            owner = Some(cell.to_string());
        } else if h.contains("due") {
            due_date = Some(cell.to_string());
        } else if description.is_none()
            && (h.contains("task") || h.contains("action") || h.contains("assignment") || h.contains("deliverable"))
        {
            description = Some(cell.to_string());
        }
    }

    if description.is_none() {
        for (header, cell) in headers.iter().zip(row.iter()) {
            let h = header.to_lowercase();
            let cell = cell.trim();
            if !h.contains("owner") && !h.contains("due") && !cell.is_empty() && !is_placeholder(cell) {
                description = Some(cell.to_string());
                break;
            }
        }
    }

    description.map(|d| ExtractedActionItem { description: d, owner, due_date })
}

/// Cap on extracted items per summary -- a sanity bound against a
/// pathologically mis-parsed table, not a real expected limit.
const MAX_ITEMS: usize = 50;

pub fn extract_action_items(markdown: &str) -> Vec<ExtractedActionItem> {
    let blocks = parse_markdown_blocks(markdown);
    let mut items = Vec::new();
    let mut in_action_section = false;
    let mut section_level: Option<u8> = None;

    for block in &blocks {
        match block {
            Block::Heading(level, text) => {
                if is_action_heading(text) {
                    in_action_section = true;
                    section_level = Some(*level);
                } else if let Some(current_level) = section_level {
                    // A heading at the same or shallower level ends the section
                    if *level <= current_level {
                        in_action_section = false;
                        section_level = None;
                    }
                }
            }
            Block::ListItem(text) if in_action_section => {
                if let Some(item) = parse_bullet_item(text) {
                    items.push(item);
                }
            }
            Block::Table { headers, rows } if in_action_section => {
                for row in rows {
                    if let Some(item) = parse_table_row(headers, row) {
                        items.push(item);
                    }
                }
            }
            _ => {}
        }
        if items.len() >= MAX_ITEMS {
            break;
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_from_lecture_style_bullets() {
        let md = "## Homework & Assignments\n\
                   - **Assignment**: Read chapter three on distributed consensus — *Due: Friday*\n\
                   - **Assignment**: Review lecture slides — *Due: no date mentioned*\n";
        let items = extract_action_items(md);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Read chapter three on distributed consensus");
        assert_eq!(items[0].due_date.as_deref(), Some("Friday"));
        assert_eq!(items[1].description, "Review lecture slides");
        assert_eq!(items[1].due_date, None, "explicit 'no date mentioned' must not become a fake due date");
    }

    #[test]
    fn extracts_from_discussion_style_bullets() {
        let md = "## Todos\n\
                   - Draft the proposal — Alice — next Monday\n\
                   - Follow up with vendor — unassigned — no date mentioned\n";
        let items = extract_action_items(md);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].description, "Draft the proposal");
        assert_eq!(items[0].owner.as_deref(), Some("Alice"));
        assert_eq!(items[0].due_date.as_deref(), Some("next Monday"));
        assert_eq!(items[1].owner, None, "'unassigned' must not become a fake owner");
        assert_eq!(items[1].due_date, None);
    }

    #[test]
    fn extracts_from_standard_meeting_style_table() {
        let md = "## Action Items\n\
                   | **Owner** | Task | Due | Reference Transcript Segment | Segment Time stamp |\n\
                   | --- | --- | --- | --- | --- |\n\
                   | Bob | Ship the release notes | 2026-09-01 | \"we need release notes\" | 04:12 |\n";
        let items = extract_action_items(md);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "Ship the release notes");
        assert_eq!(items[0].owner.as_deref(), Some("Bob"));
        assert_eq!(items[0].due_date.as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn extracts_from_project_sync_style_table_with_extra_columns() {
        let md = "## Action Items\n\
                   | **Owner** | **Task** | **Due Date** | **Priority** | **Status** |\n\
                   | --- | --- | --- | --- | --- |\n\
                   | Carol | Fix the login bug | 2026-08-15 | High | In progress |\n";
        let items = extract_action_items(md);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "Fix the login bug");
        assert_eq!(items[0].owner.as_deref(), Some("Carol"));
        assert_eq!(items[0].due_date.as_deref(), Some("2026-08-15"));
    }

    #[test]
    fn ignores_non_action_sections_even_with_tables() {
        // daily_standup's "Blockers" table and project_sync's "Milestones"
        // both use tables but aren't personal action items -- must not
        // be swept in just because a table exists somewhere in the doc.
        let md = "## Blockers\n\
                   | **Owner** | **Blocker** | Impact |\n\
                   | --- | --- | --- |\n\
                   | Dave | Waiting on API access | High |\n\
                   ## Milestones & Status\n\
                   | **Milestone** | **Status** | **ETA** |\n\
                   | --- | --- | --- |\n\
                   | Beta launch | On track | 2026-10-01 |\n";
        let items = extract_action_items(md);
        assert!(items.is_empty());
    }

    #[test]
    fn none_mentioned_placeholder_produces_no_items() {
        let md = "## Action Items\nNone mentioned\n";
        let items = extract_action_items(md);
        assert!(items.is_empty(), "a template-instructed placeholder must not become a phantom action item");
    }

    #[test]
    fn no_action_section_produces_no_items() {
        let md = "## Summary\nWe discussed the roadmap.\n## Key Decisions\n- Ship v2\n";
        let items = extract_action_items(md);
        assert!(items.is_empty());
    }

    #[test]
    fn section_ends_at_next_heading_of_same_or_shallower_level() {
        let md = "## Action Items\n- Do the thing — Alice — Friday\n## Open Questions\n- Should we — Bob — never\n";
        let items = extract_action_items(md);
        assert_eq!(items.len(), 1, "items from a later, unrelated section must not leak in");
        assert_eq!(items[0].description, "Do the thing");
    }
}
