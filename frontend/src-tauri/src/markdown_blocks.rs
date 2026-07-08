/// A deliberately simplified structural view of markdown, shared by every
/// consumer that needs to walk a generated summary programmatically rather
/// than just render it as HTML: the PDF/DOCX exporters (organization::export_pdf,
/// export_docx) and the action-item extractor (organization::action_item_extraction).
/// Inline formatting (bold/italic/links) is flattened to plain text -- summaries
/// are heading/paragraph/list/table heavy, and preserving that structure while
/// dropping inline styling is a reasonable trade for keeping every consumer simple.
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading(u8, String),
    Paragraph(String),
    ListItem(String),
    /// A markdown table: header cells, then each data row's cells. Every
    /// action-item-shaped template (standard_meeting, project_sync,
    /// retrospective, sales_marketing_client_call) renders its most
    /// important content this way, so dropping tables silently loses the
    /// content most worth exporting or extracting.
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
}

pub fn cmark_options() -> Options {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options
}

/// The app's own prompt skeleton (Template::to_markdown_structure) renders
/// section titles as a whole line of `**Title**` -- bold paragraph text,
/// not an ATX `##` heading -- and models reliably follow that convention.
/// Real pulldown_cmark parses that as Strong-wrapped-Paragraph, not
/// Tag::Heading, so every consumer that walks headings (section detection
/// for action-item extraction, heading-styled rendering in PDF/DOCX/HTML)
/// would silently miss every section unless these get normalized to real
/// headings first. Only whole-line bold matches -- a bullet like
/// "- **Assignment**: text" is untouched since the line doesn't start with
/// "**" after trimming. Public because the HTML exporter (organization::export)
/// runs markdown straight through pulldown_cmark's HTML renderer and needs
/// this same normalization pass first.
pub fn normalize_pseudo_headings(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.len() > 4
                && trimmed.starts_with("**")
                && trimmed.ends_with("**")
                && !trimmed[2..trimmed.len() - 2].contains("**")
            {
                format!("## {}", &trimmed[2..trimmed.len() - 2])
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_markdown_blocks(markdown: &str) -> Vec<Block> {
    let markdown = normalize_pseudo_headings(markdown);
    let parser = Parser::new_ext(&markdown, cmark_options());
    let mut blocks = Vec::new();
    let mut buffer = String::new();
    let mut heading_level: Option<u8> = None;
    let mut in_item = false;

    // Table state
    let mut in_table = false;
    let mut in_table_head = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut cell_buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(heading_level_to_u8(level));
                buffer.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = heading_level.take() {
                    let text = buffer.trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block::Heading(level, text));
                    }
                }
                buffer.clear();
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                buffer.clear();
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                let text = buffer.trim().to_string();
                if !text.is_empty() {
                    blocks.push(Block::ListItem(text));
                }
                buffer.clear();
            }
            Event::Start(Tag::Paragraph) => {
                buffer.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                if !in_item && !in_table {
                    let text = buffer.trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block::Paragraph(text));
                    }
                }
                buffer.clear();
            }
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table_headers.clear();
                table_rows.clear();
            }
            Event::End(TagEnd::Table) => {
                in_table = false;
                if !table_headers.is_empty() || !table_rows.is_empty() {
                    blocks.push(Block::Table {
                        headers: table_headers.clone(),
                        rows: table_rows.clone(),
                    });
                }
            }
            Event::Start(Tag::TableHead) => in_table_head = true,
            Event::End(TagEnd::TableHead) => in_table_head = false,
            Event::Start(Tag::TableRow) => current_row = Vec::new(),
            Event::End(TagEnd::TableRow) => {
                if !current_row.is_empty() {
                    table_rows.push(std::mem::take(&mut current_row));
                }
            }
            Event::Start(Tag::TableCell) => cell_buffer.clear(),
            Event::End(TagEnd::TableCell) => {
                let cell = cell_buffer.trim().to_string();
                if in_table_head {
                    table_headers.push(cell);
                } else {
                    current_row.push(cell);
                }
            }
            Event::Text(t) => {
                cell_buffer.push_str(&t);
                buffer.push_str(&t);
            }
            Event::Code(t) => {
                cell_buffer.push_str(&t);
                buffer.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak => {
                cell_buffer.push(' ');
                buffer.push(' ');
            }
            _ => {}
        }
    }

    blocks
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_paragraphs_and_lists() {
        let blocks = parse_markdown_blocks(
            "# Title\nIntro paragraph.\n## Section\n- item one\n- item two\n",
        );
        assert_eq!(
            blocks,
            vec![
                Block::Heading(1, "Title".to_string()),
                Block::Paragraph("Intro paragraph.".to_string()),
                Block::Heading(2, "Section".to_string()),
                Block::ListItem("item one".to_string()),
                Block::ListItem("item two".to_string()),
            ]
        );
    }

    #[test]
    fn parses_tables_with_headers_and_rows() {
        let md = "## Action Items\n\
                   | **Owner** | Task | Due |\n\
                   | --- | --- | --- |\n\
                   | Alice | Ship the feature | 2026-08-01 |\n\
                   | Bob | Review the PR | no date mentioned |\n";
        let blocks = parse_markdown_blocks(md);
        assert_eq!(blocks[0], Block::Heading(2, "Action Items".to_string()));
        match &blocks[1] {
            Block::Table { headers, rows } => {
                assert_eq!(headers, &vec!["Owner".to_string(), "Task".to_string(), "Due".to_string()]);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["Alice".to_string(), "Ship the feature".to_string(), "2026-08-01".to_string()]);
                assert_eq!(rows[1][0], "Bob");
            }
            other => panic!("expected a Table block, got {:?}", other),
        }
    }

    #[test]
    fn empty_table_produces_no_block() {
        let blocks = parse_markdown_blocks("Just a paragraph, no tables here.");
        assert!(blocks.iter().all(|b| !matches!(b, Block::Table { .. })));
    }

    /// The exact shape Template::to_markdown_structure's skeleton produces,
    /// and what real models (see full_session_e2e's captured llama3.1
    /// output) actually generate: whole-line "**Section**" instead of an
    /// ATX heading. Section detection must see these as real headings.
    #[test]
    fn whole_line_bold_text_becomes_a_heading() {
        let md = "**Homework & Assignments**\n\n- **Problem Set Six**: due Friday — *Due: Friday*\n";
        let blocks = parse_markdown_blocks(md);
        assert_eq!(blocks[0], Block::Heading(2, "Homework & Assignments".to_string()));
        assert_eq!(blocks[1], Block::ListItem("Problem Set Six: due Friday — Due: Friday".to_string()));
    }

    #[test]
    fn bold_text_within_a_bullet_is_not_treated_as_a_heading() {
        let md = "- **Assignment**: read chapter three\n";
        let blocks = parse_markdown_blocks(md);
        assert_eq!(blocks, vec![Block::ListItem("Assignment: read chapter three".to_string())]);
    }
}
