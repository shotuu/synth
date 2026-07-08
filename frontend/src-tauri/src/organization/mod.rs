/// Personal organization (PROJECT_BRIEF.md §5 roadmap Phase 5): the folder
/// tree, per-session tags, the cross-note action item view, and static
/// HTML export as the practical stand-in for §12's share_links (this app
/// has no server to host a live link on — see export.rs).
pub mod action_item_commands;
pub mod action_item_extraction;
pub mod action_items;
pub mod compress;
pub mod export;
pub mod export_commands;
pub mod export_docx;
pub mod export_pdf;
pub mod folder_commands;
pub mod storage;
pub mod storage_commands;
