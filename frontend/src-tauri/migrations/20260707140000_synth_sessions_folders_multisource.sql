-- Synth Phase 1: data model extension (PROJECT_BRIEF.md §3, adapted to the upstream schema)
--
-- Decisions behind this migration (2026-07-07):
--   * The brief's `notes` table is upstream's existing `meetings` table — extended
--     in place rather than creating a parallel session object.
--   * `meeting_notes` (empty and unwired upstream) already matches the brief's
--     `note_user_content`; it is reused as-is and needs no changes here.
--   * `meetings.folder_path` is the audio storage location on disk (playback,
--     retranscription, recovery) — NOT organization. It stays untouched.
--     Organizational folders are the new relational `folders` table below.
--   * Org/user tables (users, organizations, org_members, share_links) are
--     deferred per §13 — nothing here references them.

-- Notion-style nestable folder tree (personal/local, no account required)
CREATE TABLE IF NOT EXISTS folders (
    id TEXT PRIMARY KEY NOT NULL,
    parent_folder_id TEXT,
    name TEXT NOT NULL,
    icon TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (parent_folder_id) REFERENCES folders(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_folder_id);

-- Context-adaptive summary templates (PROJECT_BRIEF.md §6).
-- output_schema is JSON text defining the sections of structured output;
-- prompt_template is the LLM prompt with placeholders. Built-ins are seeded
-- by the app (Phase 3), custom rows are user-created.
CREATE TABLE IF NOT EXISTS summary_templates (
    id TEXT PRIMARY KEY NOT NULL,
    context_type TEXT NOT NULL CHECK (context_type IN
        ('meeting','lecture','discussion','coffee_chat','custom')),
    name TEXT NOT NULL,
    prompt_template TEXT NOT NULL,
    output_schema TEXT NOT NULL,
    is_builtin INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Session-object extensions on meetings (the brief's `notes` columns)
ALTER TABLE meetings ADD COLUMN context_type TEXT NOT NULL DEFAULT 'meeting'
    CHECK (context_type IN ('meeting','lecture','discussion','coffee_chat','custom'));
ALTER TABLE meetings ADD COLUMN folder_id TEXT REFERENCES folders(id);
ALTER TABLE meetings ADD COLUMN template_id TEXT REFERENCES summary_templates(id);
ALTER TABLE meetings ADD COLUMN icon TEXT;
-- Groups recurring instances (same course's lectures, same weekly standup)
ALTER TABLE meetings ADD COLUMN series_id TEXT;
-- 'auto' = summarize in the transcript's detected language
ALTER TABLE meetings ADD COLUMN summary_language TEXT NOT NULL DEFAULT 'auto';
-- JSON arrays of strings (SQLite has no array type)
ALTER TABLE meetings ADD COLUMN participants TEXT;
ALTER TABLE meetings ADD COLUMN tags TEXT;

CREATE INDEX IF NOT EXISTS idx_meetings_folder ON meetings(folder_id);
CREATE INDEX IF NOT EXISTS idx_meetings_series ON meetings(series_id);
CREATE INDEX IF NOT EXISTS idx_meetings_context_type ON meetings(context_type);

-- Extracted action items (queryable across sessions, unlike the JSON blobs
-- inside summaries)
CREATE TABLE IF NOT EXISTS action_items (
    id TEXT PRIMARY KEY NOT NULL,
    meeting_id TEXT NOT NULL,
    description TEXT NOT NULL,
    owner TEXT,
    due_date TEXT,
    done INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_action_items_meeting ON action_items(meeting_id);
CREATE INDEX IF NOT EXISTS idx_action_items_open ON action_items(done, due_date);

-- File uploads attached to a session (slides, handouts, PDFs, images).
-- extracted_text is the cached parse result fed to the summarizer.
CREATE TABLE IF NOT EXISTS note_attachments (
    id TEXT PRIMARY KEY NOT NULL,
    meeting_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    file_type TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    extracted_text TEXT,
    uploaded_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_note_attachments_meeting ON note_attachments(meeting_id);

-- Raw audio lifecycle tracking, separate from attachments (retain/delete/
-- compress have their own lifecycle; storage_path NULL = audio deleted,
-- transcript unaffected). Successor to meetings.folder_path for audio
-- location once Phase 2 wires it up.
CREATE TABLE IF NOT EXISTS note_audio (
    meeting_id TEXT PRIMARY KEY NOT NULL,
    storage_path TEXT,
    origin TEXT NOT NULL CHECK (origin IN ('recorded','imported')),
    original_file_name TEXT,
    format TEXT,
    bitrate_kbps INTEGER,
    original_size_bytes INTEGER,
    current_size_bytes INTEGER,
    retained INTEGER NOT NULL DEFAULT 1,
    last_compressed_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);
