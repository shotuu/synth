# Project Brief: Synth — a Context-Aware Speech-to-Notes App
### (name: "Synth")

A fork/extension of [Meetily](https://github.com/Zackriya-Solutions/meetily) (MIT licensed) that adds what Meetily's Community Edition lacks: (1) personal + team **organization** of notes, Notion-style, (2) **multi-source sessions** — transcript + file uploads + your own written notes, synthesized together, and (3) a **context-adaptive summarization engine** so the same recording pipeline produces the right kind of output for a work meeting, a lecture, a study group, or a coffee chat.

---

## 1. Why fork instead of build from scratch

Meetily already has, working and open source:
- Native audio capture (mic + system audio, simultaneously, no "bot" joining the call)
- Local transcription via Whisper.cpp / Parakeet
- A pluggable summarization layer (Ollama local, or Claude/Groq/OpenRouter/OpenAI via API key)
- SQLite local storage + a vector DB for semantic search
- Export to PDF/DOCX/Markdown

That's the genuinely hard, fiddly systems work (cross-platform audio capture especially). Rebuilding it would burn weeks for no differentiation. Your value-add is the **data model** (organization/sharing, multi-source sessions) and the **summarization logic** (context templates) sitting on top. Fork `Zackriya-Solutions/meetily`, then build on it.

---

## 2. Recommended architecture

| Layer | Choice | Why |
|---|---|---|
| Desktop shell | Tauri (Rust) + Next.js — inherited from Meetily | Native audio capture requires this; don't swap it out |
| Local DB | SQLite (inherited) | Offline-first; source of truth when not connected to an org |
| Local vector store | Inherited vector DB (for semantic search across your own notes) | |
| Transcription | Faster-Whisper (large-v3 / turbo) as default, Parakeet as a speed option — both already integrated in Meetily | See §7 for the full reasoning — Whisper's ecosystem and cross-platform CPU/Metal support make it the practical default over higher-accuracy models that need a real GPU |
| Speaker diarization | New: pyannote.audio (via a WhisperX-style pipeline) added on top of the existing transcription step | Free, open source, the de facto standard — not in Meetily today; see §7 |
| Summarization | Pluggable: Ollama (local) **or** Claude API / OpenAI / Groq (cloud), selectable per-note or globally in settings | Already how Meetily is built — extend the provider abstraction to also carry your context templates and multi-source input |
| File storage (attachments) | Local filesystem under the app's data dir, referenced by path in SQLite; synced to object storage (or just the Postgres bytea/blob) only when a note is shared to an org | Keeps personal mode fully local; avoids building a blob-storage service for the common case |
| Team/org backend | New: FastAPI service (Python — matches Meetily's existing backend) + PostgreSQL | **Optional, and skippable entirely.** Only needed if/when you actually want a note shared to an org workspace |
| Sync model | **Local-first, sync-on-share.** Personal notes never leave SQLite unless explicitly shared. Sharing pushes a copy (transcript + attachments + summary) to Postgres via the backend API. | Keeps the privacy story Meetily is known for, while still enabling team features later |
| Auth (org mode only) | JWT-based, simple email/password or OAuth | Only needed once org sharing is used; personal mode requires no login |

**Key architectural decision to state explicitly to Claude Code:** personal-mode notes are fully offline and require no account. Org-mode is opt-in per workspace. Don't make auth a hard requirement for the core recording/transcription/summarization loop.

**On your storage question, directly: yes — the entire app can run on local storage only, indefinitely, with zero hosting cost.** Postgres/FastAPI (Phases 7–8 below) exist purely to support sharing a note to a team. If you never build or enable that, nothing about the personal-mode app changes or breaks — there's no "free tier that runs out" or database to pay for, because there's no database beyond the SQLite file already sitting on your disk. You can build and use this as a fully local app for as long as you want and add the org backend only if/when you actually need it. See §4 for how to think about the local storage footprint itself, since that's the part that actually grows with usage.

---

## 3. Data model

```sql
-- Users & orgs (only relevant once org-sharing is used)
CREATE TABLE users (
  id UUID PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  display_name TEXT NOT NULL,
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE organizations (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT UNIQUE NOT NULL,
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE org_members (
  org_id UUID REFERENCES organizations(id),
  user_id UUID REFERENCES users(id),
  role TEXT CHECK (role IN ('owner','admin','editor','viewer')),
  PRIMARY KEY (org_id, user_id)
);

-- Personal organization structure (exists locally even with no account)
-- This is your Notion-style tree: folders can nest, and can contain sub-folders or notes
CREATE TABLE folders (
  id UUID PRIMARY KEY,
  parent_folder_id UUID REFERENCES folders(id), -- nesting, nullable
  org_id UUID REFERENCES organizations(id),      -- NULL = personal/local
  name TEXT NOT NULL,
  icon TEXT,                                      -- emoji or icon key, Notion-style
  sort_order INT DEFAULT 0,
  created_at TIMESTAMP DEFAULT now()
);

-- Core session/note object — a single meeting, lecture, or chat
CREATE TABLE notes (
  id UUID PRIMARY KEY,
  folder_id UUID REFERENCES folders(id),
  org_id UUID REFERENCES organizations(id),      -- NULL = personal
  owner_id UUID REFERENCES users(id),             -- NULL = local anonymous user
  title TEXT NOT NULL,
  icon TEXT,
  context_type TEXT CHECK (context_type IN
    ('meeting','lecture','discussion','coffee_chat','custom')) NOT NULL,
  template_id UUID REFERENCES summary_templates(id),
  visibility TEXT CHECK (visibility IN ('private','shared')) DEFAULT 'private',
  series_id UUID,            -- NULL for one-off sessions; groups recurring instances
                              -- (same weekly standup, same course's lecture series) — see §12
  summary_language TEXT DEFAULT 'auto', -- 'auto' = match detected transcript language
  recorded_at TIMESTAMP,
  duration_seconds INT,
  participants TEXT[],       -- freeform names/identifiers
  tags TEXT[],
  created_at TIMESTAMP DEFAULT now()
);

-- Lightweight sharing: a viewable link that doesn't require the recipient to have
-- an account or belong to an org — see §12 for why this matters alongside org sharing
CREATE TABLE share_links (
  id UUID PRIMARY KEY,
  note_id UUID REFERENCES notes(id),
  token TEXT UNIQUE NOT NULL,        -- unguessable slug, e.g. yourapp.local/n/abc123
  expires_at TIMESTAMP,              -- optional expiry
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE transcript_segments (
  id UUID PRIMARY KEY,
  note_id UUID REFERENCES notes(id),
  speaker_label TEXT,        -- "Speaker 1", or resolved name once diarization + naming is added
  start_ms INT,
  end_ms INT,
  text TEXT NOT NULL
);

-- NEW: file uploads attached to a session (slides, handouts, docs, images)
CREATE TABLE note_attachments (
  id UUID PRIMARY KEY,
  note_id UUID REFERENCES notes(id),
  file_name TEXT NOT NULL,
  file_type TEXT NOT NULL,        -- 'pdf', 'docx', 'pptx', 'image', 'txt', etc.
  storage_path TEXT NOT NULL,     -- local path, or object storage key once synced
  extracted_text TEXT,            -- cached text extraction, fed into the summarizer
  uploaded_at TIMESTAMP DEFAULT now()
);

-- NEW: the user's own freeform written notes for a session (separate from the transcript)
CREATE TABLE note_user_content (
  note_id UUID PRIMARY KEY REFERENCES notes(id),
  content_json JSONB NOT NULL,    -- rich-text/block editor state (e.g. TipTap JSON)
  content_markdown TEXT,          -- flattened plain-text/markdown version, fed into the summarizer
  updated_at TIMESTAMP DEFAULT now()
);

-- NEW: raw audio tracking — separate from note_attachments since it has its own
-- lifecycle (retain/delete/compress) and can originate from recording OR import
CREATE TABLE note_audio (
  note_id UUID PRIMARY KEY REFERENCES notes(id),
  storage_path TEXT,              -- NULL once deleted, transcript is unaffected
  origin TEXT CHECK (origin IN ('recorded','imported')) NOT NULL,
  original_file_name TEXT,        -- set when origin = 'imported'
  format TEXT,                    -- 'wav', 'opus', 'mp3', etc.
  bitrate_kbps INT,
  original_size_bytes BIGINT,
  current_size_bytes BIGINT,
  retained BOOLEAN DEFAULT true,
  last_compressed_at TIMESTAMP,
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE summaries (
  id UUID PRIMARY KEY,
  note_id UUID REFERENCES notes(id),
  template_id UUID REFERENCES summary_templates(id),
  structured_output JSONB NOT NULL,  -- see template schemas below
  sources_used TEXT[],               -- e.g. ['transcript','attachments','user_notes']
  generated_by TEXT,                 -- 'ollama:llama3', 'claude-sonnet-5', etc.
  created_at TIMESTAMP DEFAULT now()
);

CREATE TABLE action_items (
  id UUID PRIMARY KEY,
  note_id UUID REFERENCES notes(id),
  description TEXT NOT NULL,
  owner TEXT,           -- freeform name, or user_id if resolvable
  due_date DATE,
  done BOOLEAN DEFAULT false
);

CREATE TABLE summary_templates (
  id UUID PRIMARY KEY,
  org_id UUID REFERENCES organizations(id), -- NULL = built-in/global template
  context_type TEXT NOT NULL,
  name TEXT NOT NULL,
  prompt_template TEXT NOT NULL,   -- the actual LLM prompt, with placeholders
  output_schema JSONB NOT NULL,    -- defines the sections in structured_output
  is_builtin BOOLEAN DEFAULT false
);
```

Notes:
- `notes.org_id IS NULL` → lives only in local SQLite, never touches the backend.
- Sharing a note = it gets an `org_id`, `visibility='shared'`, and is pushed to Postgres along with its `note_attachments` and `note_user_content`.
- `summary_templates` being a table (not hardcoded) is what makes "custom" context types possible later.
- `note_user_content` is 1:1 with a note for v1 (one notes document per session). If you later want true Notion-style multi-page nesting under a session, promote this to its own row-per-block table — don't build that up front, it's a real complexity jump for little v1 benefit.
- `note_audio.storage_path IS NULL` just means the raw recording was deleted — the transcript in `transcript_segments` is untouched, since transcription already happened. This is what makes "delete the audio, keep the transcript" a safe, reversible-in-spirit action (you lose the ability to re-transcribe or listen back, but nothing else breaks).

---

## 4. Local storage footprint — what actually grows, and how to manage it

Since everything is local (§2), the real constraint isn't cost, it's disk space on your machine. Worth designing for deliberately since you're right that transcripts + files + audio across many sessions add up:

**Where things live.** Use the OS-standard app data directory (e.g. `~/Library/Application Support/Synth` on macOS, `%APPDATA%/Synth` on Windows — Tauri gives you this path natively), structured like:
```
Synth/
  db/scrybe.sqlite          <- tiny, even with thousands of sessions (text is cheap)
  attachments/<note_id>/    <- uploaded files, as-is
  audio/<note_id>/          <- raw recordings, IF retained (see below)
  vector_index/             <- local embedding index for search
```

**Text is not your storage problem.** Transcripts, summaries, and your written notes are plain text — even a semester's worth of lecture transcripts is a few MB total. SQLite comfortably handles tens of thousands of sessions. Don't over-engineer this part.

**Raw audio is your storage problem.** An hour of recorded audio is tens of MB even compressed, and if you're recording lectures + meetings regularly, that adds up fast over a semester. Decide this explicitly rather than defaulting to "keep everything forever":
- **Recommended default:** once transcription (and diarization, if used) completes successfully, delete the raw audio automatically, keeping only the transcript. Make this a setting, not a hardcoded behavior — some people want to keep audio for ambiguous transcriptions or for re-processing with a better model later.
- **If keeping audio:** compress it (Opus at a low bitrate, ~16–24kbps mono, is effectively transparent for speech and dramatically smaller than raw PCM or even MP3), and consider an auto-delete-after-N-days policy for anyone who wants a middle ground between "delete immediately" and "keep forever."

**Uploaded files (slides, PDFs)** are typically small individually; the main thing to avoid is silently duplicating the same file across sessions if a user re-uploads the same slide deck. Not worth solving in v1 — just don't actively make it worse by re-encoding or upscaling anything on ingest.

**Free multi-device backup, without building sync yourself:** if you eventually want your notes available across two machines without standing up the org/Postgres backend, the simplest zero-cost option is letting the user point the app's data directory at a folder that's already synced by something they likely already have — iCloud Drive, Dropbox, or Google Drive's desktop sync. That gives you cross-device backup for free, without writing a sync layer.

One real gotcha if you do this: **don't put the live SQLite file inside the actively-synced folder.** Cloud sync clients (especially Dropbox and iCloud) can grab a database file mid-write and either corrupt it or create conflicted duplicate copies, since SQLite does incremental writes to the same file rather than write-once. The `attachments/` and `audio/` folders are safe to sync (files are written once and not modified after). For the database itself, either keep it outside the synced folder and rely on periodic export/backup instead, or use SQLite's WAL mode carefully and accept the small risk — worth flagging to Claude Code explicitly so it doesn't casually put the `.sqlite` file inside a Dropbox-managed path.

### Storage Manager — an in-app cleanup tool

Don't leave storage cleanup as "go delete files in Finder/Explorer" — that's exactly the kind of thing that should be a first-class screen, given `note_audio` (§3) already tracks per-session audio size and retention state. A dedicated Storage Manager view should give the user:

- **A sortable list of sessions by audio size**, with columns for context type, date, folder, current audio size, and retention status — so the biggest space users are easy to find, not buried alphabetically in a folder tree.
- **Bulk selection with three actions**, not just delete:
  1. **Delete audio, keep transcript** — sets `note_audio.storage_path = NULL`, `retained = false`. The session and its summary are untouched.
  2. **Compress audio** — re-encodes the retained file to a lower-bitrate Opus, updates `current_size_bytes`/`bitrate_kbps`/`last_compressed_at`. This is the "can't afford to delete it but want to save space" option — good default suggestion: compress anything still at its original recorded bitrate that hasn't been touched in 30+ days.
  3. **Delete entire session** — the fully destructive option (transcript, summary, attachments, everything), kept clearly visually distinct from option 1 so it's hard to hit by accident.
- **A simple "suggested cleanup" surface** — e.g. sessions with retained audio older than N days that haven't been reopened — rather than making the user hunt for candidates manually. This can just be a filtered/sorted view of the same list, not a separate recommendation engine.
- **Aggregate stats at the top** (total app storage used, split by audio/attachments/database) so the user can see whether cleanup is actually worth doing before diving in.

This is a good fit for Phase 6 in the roadmap below — it needs the note list UI and folder structure from Phase 5 to already exist, and benefits from there actually being some sessions to manage.

---

## 5. Multi-source sessions — how everything comes together

Each session (`note`) is a container, not just a transcript. It can have, in any combination:
1. **A transcript** — from a recorded meeting/lecture (optional; some sessions might be notes-only, e.g. you paste in a PDF and just want it summarized)
2. **File uploads** — slides, handouts, PDFs, images — parsed into `extracted_text` on upload
3. **Your own written notes** — a block/rich-text editor, saved as `note_user_content`

**The context assembler:** before calling the summarizer, a service layer gathers all available sources for a session:
```
transcript_text   = concatenated transcript_segments, with speaker labels
attachments_text  = concatenated note_attachments.extracted_text, labeled by filename
user_notes_text   = note_user_content.content_markdown
```
These get combined into a single prompt alongside the selected `summary_templates.prompt_template`, so the model is explicitly told which parts came from the recording, which came from uploaded material, and which are your own notes — and asked to reconcile them (e.g., "the slides mention X, the professor also emphasized X verbally, here's the combined point" rather than repeating it twice). Store which sources were actually used in `summaries.sources_used` so the UI can show "this summary used your transcript + 2 files + your notes."

**File parsing needed on upload:**
- PDF → text extraction (PyMuPDF or similar; OCR fallback for scanned/image-only PDFs)
- DOCX/PPTX → text extraction
- Images → OCR only if there's clearly text in them (e.g., a photographed whiteboard); don't run OCR on every image indiscriminately, it's wasted compute and noisy output

**Practical note on context length:** for long lecture transcripts + multiple slide decks, you'll exceed a reasonable prompt size. Chunk long sources and summarize incrementally, or use a model with a large context window for the final synthesis pass — don't just truncate silently, since that would drop material without telling the user.

### Importing existing audio files (not just recording live)

Worth calling out: Meetily already shipped this upstream (v0.3.0 added audio file import and retranscription), so this isn't new engineering — it's wiring an existing entry point into your new pipeline. A session's transcript can originate from either:
1. **Live recording** — the existing mic + system audio capture flow, or
2. **Imported audio file** — the user picks or drags in an existing recording (wav/mp3/m4a/ogg)

Both should converge on the same downstream pipeline: transcode to 16kHz mono if needed (ffmpeg — already in Meetily's pipeline), run through faster-whisper, run through diarization, auto-classify context type, then let the user attach files/write notes and generate a summary exactly as if it had been recorded live. Don't build import as a separate, parallel feature — it should just be a second way to populate `transcript_segments` and `note_audio` for a session.

**Decide up front:** when a file is imported, do you copy it into the app's own `audio/<note_id>/` storage, or just store a reference to wherever the original file already lives? Copying is the safer default (the session doesn't break if the user later moves or deletes the original, and your Storage Manager retention/compression actions work uniformly regardless of origin), at the cost of duplicating disk space temporarily. For someone importing a large batch of old recordings at once, referencing in place might be worth offering as an advanced option — but default to copy, and treat this as one more thing to decide explicitly rather than let it fall out of whatever's easiest to implement first.

Batch import (drop a whole folder of past lecture recordings at once) is a natural extension of single-file import — same pipeline, just queued with a progress list — worth deferring to a later phase rather than building alongside the initial single-file import.

---

## 6. The core differentiator: context-adaptive summarization

Five built-in templates. Each defines a distinct `output_schema` for the summarizer to fill in — this is what actually changes based on context, not just tone.

### Template: `meeting`
```json
{
  "attendees": ["string"],
  "agenda_covered": ["string"],
  "key_decisions": ["string"],
  "action_items": [{"description": "string", "owner": "string", "due_date": "string|null"}],
  "open_questions": ["string"],
  "next_steps": "string"
}
```

### Template: `lecture`
```json
{
  "course_or_topic": "string",
  "key_concepts": [{"term": "string", "definition": "string"}],
  "worked_examples": ["string"],
  "professor_emphasis": ["string"],
  "homework_or_assignments": [{"description": "string", "due_date": "string|null"}],
  "exam_relevant_flags": ["string"],
  "suggested_followup_reading": ["string"]
}
```

### Template: `discussion` (study groups, project syncs, brainstorms)
```json
{
  "topic": "string",
  "key_points_raised": ["string"],
  "decisions_if_any": ["string"],
  "todos": [{"description": "string", "owner": "string|null", "due_date": "string|null"}],
  "open_threads": ["string"]
}
```

### Template: `coffee_chat` (casual, networking, catch-ups)
```json
{
  "who_you_spoke_with": "string",
  "topics_discussed": ["string"],
  "personal_details_worth_remembering": ["string"],
  "potential_followups": ["string"],
  "favors_or_intros_offered": ["string"]
}
```

### Template: `custom`
User/org supplies their own `prompt_template` + `output_schema` via the UI. The summarization service just needs to treat this generically — same code path as the built-ins, no special-casing.

**Auto-classification:** after transcription finishes, run a cheap classification pass (a short local-model or heuristic call, not the same as the full summarization call) that suggests a `context_type` from the transcript content and lets the user confirm/override before the final summary is generated.

---

## 7. Speech-to-text and speaker diarization — what's actually best right now

You asked whether Meetily's current setup (Whisper.cpp / Parakeet) is still the best free option, and whether speaker identification is achievable. Short answer: keep Whisper as the backbone, but run it through **faster-whisper** rather than vanilla whisper.cpp where you can, and add **pyannote.audio** for diarization — Meetily doesn't have that yet, and it's genuinely free and well-documented.

**On raw transcription accuracy**, the leaderboard has moved past Whisper — NVIDIA's Canary-Qwen 2.5B and IBM's Granite Speech 3.3 8B currently post lower word-error rates on the standard Hugging Face Open ASR leaderboard, and Alibaba's Qwen3-ASR (released Jan 2026) and Mistral's Voxtral (Feb 2026, Apache 2.0) are newer, strong open-weight entrants. None of them are the right pick for this app, though:
- Canary-Qwen and Granite need real NVIDIA GPU infrastructure and ML tooling maturity that doesn't fit a personal laptop app
- Voxtral only covers 13 languages and its on-device tooling is still young
- Qwen3-ASR's community tooling (packaging, bindings) is also still catching up to Whisper's

Whisper (specifically **large-v3** or **large-v3-turbo**) remains the practical choice for an app like this because of its 99+ language support, MIT license, and — critically — the maturity of whisper.cpp/faster-whisper for running well on ordinary CPUs and Apple Silicon (Metal), which is what your users' laptops actually are. **Faster-Whisper** (a CTranslate2-based reimplementation) gets you meaningfully faster inference and lower memory than vanilla whisper.cpp at identical accuracy, so it's worth using in place of, or alongside, Meetily's existing whisper.cpp integration. Parakeet stays useful as the "speed over accuracy" option Meetily already offers.

**On speaker diarization** — this is very achievable and worth building, since Meetily doesn't ship it yet (upstream calls it "coming soon," gated to their paid tier). The standard open-source approach in 2026 is:
- **pyannote.audio** (currently `speaker-diarization-community-1`, CC-BY-4.0 licensed, free) is the de facto standard for "who spoke when." It requires a free Hugging Face account and accepting the model's terms — not a paywall, just a click-through.
- **WhisperX** wraps faster-whisper + pyannote + word-level alignment into one pipeline, so you don't have to hand-roll the integration — it's the fastest path to "transcript with SPEAKER_00/SPEAKER_01 labels."
- Realistic accuracy expectations: strong (90–95%) for 2–3 clean speakers, dropping to 80–88% for 4–6 speakers, and noticeably worse with heavy crosstalk/overlapping speech. That's a real limitation for large group meetings, but it's fine for the 1-on-1s, small study groups, and lecture-Q&A scenarios you described.
- After diarization gives you `SPEAKER_00`/`SPEAKER_01` labels, let the user map those to real names once per session (or once per recurring participant) — that's the "identify different people talking" feature you want, and it's a UI/UX problem on top of a solved diarization problem, not a hard ML problem.
- Diarization is meaningfully slower on CPU-only machines; if a user has an NVIDIA GPU, run it there, otherwise it'll add real processing time after the recording ends — surface that as a background/progress step rather than blocking the UI.

**Bottom line for the spec:** Whisper (via faster-whisper) stays your default transcription engine, Parakeet stays your speed option, and pyannote/WhisperX-style diarization is a new, genuinely free addition worth prioritizing — it directly answers your "identify who's talking" ask.

---

## 8. UI/UX direction

You want this to feel good to use, not like a generic internal tool. A few concrete directions to hand to Claude Code (or a design pass) rather than leaving "make it look nice" implicit:

- **Structure like Notion, read like a document.** Left sidebar = folder tree (nestable, icons, drag-to-reorder). Main pane = a single scrollable session page, not a tabbed interface — transcript, uploaded files, your notes, and the generated summary all live as sections on one page, the way a Notion page holds multiple blocks. Tabs would fragment something that's conceptually one document.
- **Give the summary visual priority.** It's the payoff of the whole session — treat it as the top of the page (or a pinned/collapsible panel), with the raw transcript and source material available below/alongside for verification, not competing for the same visual weight.
- **Speaker color-coding as the signature element.** Once diarization is in, assigning each speaker a consistent color (used in the transcript, in name tags, in the summary's attendee list) is a small, distinctive touch that makes multi-speaker sessions genuinely easier to scan — this is a good candidate for the one "signature" visual idea worth being deliberate about, rather than defaulting to a generic AI-tool look (cream background + serif + terracotta accent is the current cliché to actively avoid).
- **Context-type badges.** Since context_type drives the summary shape, make it visually obvious on every session card/list item (a small colored tag: Meeting / Lecture / Discussion / Coffee Chat) so scanning a folder full of sessions is fast.
- **Empty and loading states matter here specifically** because transcription and summarization aren't instant — show real progress ("Transcribing… 4 of 12 min processed," "Identifying speakers…", "Generating summary…") rather than a generic spinner, since these steps can take real time locally.
- **Command palette (Cmd/Ctrl+K)** for jumping between folders/sessions — a small addition that makes a Notion-style tree feel fast rather than click-heavy.
- **Dark mode** — Meetily already has this, keep it.
- **Copy/microcopy:** name things by what the user controls, not how the system works — "Add to this session," not "Ingest source." Buttons should describe the result ("Generate summary"), and once clicked, later confirmations should use the same word ("Summary generated"), not a synonym.

If you're building this UI with Claude Code and have design-focused tooling available, it's worth asking it to take one real aesthetic risk on the speaker-color or context-badge system specifically, then keep everything else disciplined and quiet around it — a maximalist treatment everywhere would fight the "read like a document" goal above.

---

## 9. Feature phases (build in this order — don't ask Claude Code to do all of this in one shot)

**Phase 0 — Fork & orient**
Clone `Zackriya-Solutions/meetily`, get it building locally, understand its existing backend/frontend structure before changing anything.

**Phase 1 — Data model extension**
Add `context_type`, `folders`, `summary_templates`, `action_items`, `note_attachments`, `note_user_content`, `note_audio` tables to the local SQLite schema as a new sqlx migration (upstream's existing migration pattern). Reconcile against what already exists rather than duplicating it — notably, replace the existing flat `folder_path` column with the relational `folders` table (§13), and check whether `meeting_notes` can be renamed/reused directly as `note_user_content` instead of creating a parallel table. Keep org/user/postgres tables separate — those come in Phase 7 (currently deferred, see §13).

**Phase 2 — Multi-source sessions**
Build file upload + parsing (PDF/DOCX/PPTX text extraction, selective OCR), the written-notes block editor, audio file import as a second entry point alongside live recording (§5), and the context-assembler service described in §5. At this point a session can hold a transcript (recorded or imported), files, and notes independently of summarization.

**Phase 3 — Context-adaptive summarization**
Per Phase 0 findings: upstream already has a JSON-based summary template system (six built-ins, custom-template loader) and summary-language detection, so this phase is narrower than originally scoped — it's really about mapping the brief's 5 context-type templates (§6) onto the existing template mechanism, enforcing structured JSON output per template, wiring in the context-assembler from Phase 2, and adding the auto-classification step. Confirm what the existing six built-ins actually cover before writing new ones — some may already be close enough to adapt rather than replace.

**Phase 4 — Speaker diarization**
Integrate pyannote.audio (WhisperX-style pipeline) into the existing transcription flow. Add the UI for mapping SPEAKER_00/01 labels to real names.

**Phase 5 — Personal organization UI**
Folder tree, drag-to-reorder, tags, note list with filters by context_type/tag/date, search (reuse Meetily's existing vector DB), command palette, a cross-note action item view (all open todos across every session, filterable by folder/date — no new tables needed, just a query against existing `action_items`), and share-link generation (§12) for casual read-only sharing without full org auth.

**Phase 6 — Storage Manager**
Build the cleanup tool described in §4: sortable session list by audio size, bulk delete-audio/compress/delete-session actions, aggregate storage stats. This needs the note list UI from Phase 5 to already exist.

**Phase 7 — Org/team backend** *(deferred — see §13's resolution to skip this for v1 in favor of `share_links`; only pick this up if you end up actually needing persistent team membership/permissions)*
Stand up the FastAPI + Postgres service. Auth, org creation, invites, roles. Implement "share this session to an org" (pushes transcript + attachments + summary to Postgres).

**Phase 8 — Shared workspace UI** *(deferred alongside Phase 7)*
Org switcher, shared folder views, permission-gated editing, activity/member list.

**Phase 9 — Polish**
Export formats (inherit Meetily's PDF/DOCX/MD export, extend to include structured summary sections + attached files), settings for AI provider selection per feature, custom template editor UI, visual polish pass per §8.

---

## 10. Non-functional requirements to state up front

- **Privacy default:** personal notes never leave the device unless explicitly shared. No silent cloud sync.
- **Offline-first:** recording, transcription, and local-model summarization must all work with zero network connection (this matters for your lecture use case — don't assume classroom wifi).
- **No meeting bot:** inherit Meetily's approach of capturing system audio directly rather than joining as a call participant.
- **Consent:** surface a reminder about recording-consent laws in the UI before starting a recording (Meetily does this too).

---

## 11. How to actually use this with Claude Code

Don't paste this whole document as one giant instruction. Feed it phase by phase, and let Claude Code ask you clarifying questions at each stage. A good pattern:

1. Save this file as `PROJECT_BRIEF.md` in the repo root once you've forked Meetily, so Claude Code has it as persistent context.
2. For each phase, give a focused prompt referencing the brief. Example for Phase 3:

```
Read PROJECT_BRIEF.md, specifically section 6 (context-adaptive summarization)
and section 5 (multi-source sessions). We're on Phase 3 of the roadmap in
section 9, and Phase 2 (file uploads, notes editor, context-assembler) is
already built and working.

Implement the summary_templates table and seed it with the 5 built-in templates
from section 6. Extend the existing summarization service to:
1. Accept a template_id and load its prompt_template + output_schema
2. Call the context-assembler from Phase 2 to gather transcript + attachments
   + user notes for the session
3. Call the existing pluggable LLM provider (Ollama or API) with the
   template's prompt and assembled context, requesting JSON matching output_schema
4. Store the result in the summaries table as structured_output, along with
   which sources were actually used

Add the auto-classification step: after transcription completes and before
summarization, make a lightweight classification call to suggest a
context_type, and surface it in the UI for user confirmation.

Don't touch diarization or org/team tables yet — those are Phases 4 and 6.
Ask me before making schema changes outside what's described above.
```

3. Always end a phase by asking Claude Code to run/test what it built before moving to the next phase — don't stack unverified phases.

---

## 12. Stakeholder-driven gaps — what a student vs. a working professional would each want

Worth stress-testing the spec against a couple of concrete personas before you start building, since the current design leans on "personal notes + optional team sharing" as the only axis, and that's not quite how either group actually works.

### As a student (your own primary use case)

- **A course is bigger than a folder.** A folder holds sessions, but a student thinks in terms of "CS33, this semester" — a recurring container where every Tuesday's lecture is a new session but they all belong to one continuous thread. The `series_id` column added to `notes` above exists for this: link every instance of the same recurring course/standup/study group together, so the app can (eventually) show "here's how this has evolved" rather than treating each session as fully isolated.
- **Exam prep needs a rollup, not just per-lecture summaries.** Nobody studies from twelve separate lecture summaries — they want "everything this course covered, synthesized." This is a genuinely different feature from per-session summarization: a **collection-level summary** that takes several sessions' summaries (not full transcripts, to keep it cheap) as input and produces one study-guide-shaped output. Worth prototyping once you have a few real courses' worth of sessions to test against — don't build it speculatively before then.
- **Homework shouldn't be trapped inside individual note pages.** `action_items` already has everything needed (it's keyed by `note_id`, joinable to `notes` for course/date) — a **cross-note action item view** ("all your open todos across every class, this week") is a UI-only addition on data you already have. Cheap, high value, worth folding into Phase 5.
- **Sharing with a study group isn't "joining an org."** Three classmates wanting to see your notes shouldn't require them to create accounts and you to manage roles — that's the `organizations`/`org_members` model, built for actual teams. The `share_links` table added above is the lighter-weight answer: a single unguessable read-only link, no recipient account needed. Genuinely worth building **before** the full org/Postgres backend (Phase 7) rather than after — it covers the casual-sharing case most students actually have, and it's a fraction of the effort of full org auth. Worth reconsidering whether Phase 7 (org backend) is even necessary for your own use, versus link-sharing alone.
- **Cost-consciousness as a real constraint, not just a preference.** If a student defaults to a cloud provider (Claude/OpenAI) for summarization and runs a semester of hour-long lecture transcripts through it, that's a real recurring cost they may not be tracking. Worth showing an estimated cost (or at least a token/length warning) before running a cloud summarization pass on a long transcript, not just leaving the provider choice as a silent setting.
- **Phone-recorded audio is a real workflow.** A student who records a lecture on their phone (because bringing a laptop to every class is impractical) needs the import feature from §5 as a first-class path, not an afterthought — worth explicitly testing "record on phone voice memo app, AirDrop/transfer to laptop, import" as a real user journey.

### As a working professional

- **Meetings come from a calendar, not thin air.** Manually typing a title and attendee list for every recorded meeting is friction a calendar integration (Google Calendar/Outlook) removes — auto-filling title, attendees, and even a sensible default `context_type` of `meeting`. This is real new scope (OAuth, event matching), so it belongs as a later, optional phase rather than something to build alongside the core loop — but worth deciding now whether it's in scope at all, since it changes how much you invest in manual metadata-entry UI in the meantime.
- **Recurring meetings want continuity, not a blank slate each time.** A weekly 1:1 or standup benefits from "here's what was still open last time" surfacing automatically in the new session's summary — this is what `series_id` is for: the summarizer can optionally pull the previous instance's open `action_items` into the new session's context.
- **This app shouldn't be where action items go to die.** A professional's real task system is Todoist/Asana/Jira/email, not a notes app. Rather than building deep two-way integrations (real scope, real maintenance burden), a simpler and still valuable v1 answer is a clean **export** of a session's action items (markdown, or a structured format another tool can import) — full integrations can be a later, optional phase if it turns out to matter.
- **Confidentiality is a policy question, not just a UI reminder.** Beyond the recording-consent reminder already in §10, some professional contexts (legal, healthcare, anything regulated) may need an org-level policy that *forces* local-only AI processing for everyone in that org, rather than leaving cloud-vs-local as each member's individual choice. Worth deciding whether `organizations` needs a `require_local_ai_only BOOLEAN` flag before you build the org backend, since retrofitting an enforced policy after members have already been using cloud providers is messier than starting with it.
- **One person, multiple separate orgs.** A consultant or board member likely belongs to more than one organization (their employer, a client, a nonprofit) that must never see each other's data. The `org_members` many-to-many model already supports this, but it's worth explicitly testing for strict tenant isolation (no query ever accidentally joins across `org_id` boundaries) once Phase 7 is built, and giving the UI an obvious "which org am I in right now" indicator — easy to get subtly wrong, expensive to get wrong in production.

### Cross-cutting, regardless of persona

- **Trust in the summary matters more than any single feature.** Whisper and LLMs both hallucinate — Whisper on silence/noise, LLMs on ambiguous source material. Two cheap mitigations worth baking into every template's prompt from the start: instruct the model to only include what's explicitly supported by the source material and mark anything genuinely unclear rather than guessing (especially important for the `homework_or_assignments` and `action_items` fields — a fabricated due date is a worse failure than a missing one), and make every generated summary **editable in place** rather than read-only, so a wrong field is a quick fix, not a reason to distrust the whole feature.
- **Output language shouldn't be assumed to be English.** The `summary_language` column added above defaults to matching whatever language the transcript itself is in — a French lecture should produce a French summary by default, not force a translation nobody asked for. Worth deciding whether "auto" is really the right default versus a fixed user preference.
- **Live captions double as an accessibility feature, nearly for free.** The live transcript view you're already building for real-time feedback during recording is also, for a hard-of-hearing user sitting in a lecture, a genuinely useful captioning tool — worth deliberately designing that view to be legible and prominent (large, high-contrast, easy to glance at) rather than treating it as a debug-style side panel, since it's serving double duty.

**What to actually prioritize from this list:** the cross-note action item view and `series_id`/`share_links` schema additions are cheap and worth including now (they're already reflected in §3's schema above). Calendar integration, deep task-manager sync, and collection-level rollup summaries are real, valuable, and genuinely more scope — treat them as an explicit "Phase 10+, once the core loop is solid" bucket rather than something to chase before v1 is usable.

---

## 13. Open decisions — resolved defaults, and what's still genuinely yours to pick

Most of these have a clear answer once you frame this as primarily a personal tool (your studying, your internship notes, maybe casual sharing with classmates) rather than something being deployed to a company. Resolving them now so Claude Code isn't guessing mid-build:

**Resolved — go with these unless you have a specific reason not to:**
- **Bundle identifier: change it immediately (Phase 1), don't defer it.** Forking Meetily without changing `tauri.conf.json`'s identifier means dev Synth and any installed production Meetily share the same app data directory and the same live SQLite file — a dev migration can silently mutate the production app's database out from under it. Change the identifier (e.g. `com.synth.app`) before Phase 2, and if a shared database has already been migrated in place, restore the production app's copy from a pre-migration backup rather than leaving it on a schema its binary doesn't know about.
- **Full org backend (Phase 7):** skip it for v1. Build `share_links` (§12) instead — it covers casual sharing with classmates or coworkers without the cost of standing up Postgres/auth/roles for a user base of effectively one. Revisit Phase 7 only if you're ever actually running this for a team that needs persistent membership and permissions.
- **Org-level compliance policy (`require_local_ai_only`):** moot if you're skipping the org backend — drop it from scope entirely for now.
- **Custom template UI exposure:** also moot without an org backend — for a single user, there's no "admin vs. member" distinction to design around. You get full prompt-editing access to your own templates by default.
- **Audio retention default:** delete raw audio after successful transcription, keep the transcript. Make it a toggle (§4), but this is the right default.
- **Imported audio — copy vs. reference:** copy into app storage. Simpler, safer, and consistent with how Storage Manager treats everything else.
- **Block editor library:** BlockNote, not TipTap — reversed from the earlier pick once Phase 0 orientation found BlockNote already integrated upstream (used in summary views) with an unused `meeting_notes` table that maps directly to `note_user_content`. Standardize on what's already there rather than introducing a second editor dependency.
- **Folders: relational table, not the existing flat `folder_path` column.** Upstream stores folder as a flat string on meetings. Keep the brief's original `folders` table (parent_folder_id, icon, sort_order) instead — the Notion-style tree in §8 (nesting, icons, drag-to-reorder) needs real referential structure, not a path string. Decide this in Phase 1 before other code assumes the flat column.
- **Phase 4 diarization delivery mechanism:** upstream archived its Python/FastAPI backend — transcription, LLM providers, and storage are now Rust-native. pyannote.audio is Python-only, so it'll need a sidecar process analogous to the existing `llama-helper` pattern rather than slotting into an existing service. Not a Phase 1 blocker, just flagged so Phase 4 doesn't assume a Python backend that no longer exists.
- **Diarization compute:** CPU-only fallback (transcription without speaker labels) should always work; treat GPU acceleration — NVIDIA *or* Apple Silicon via PyTorch's MPS backend, since you're likely on a Mac — as a speed bonus, not a hard requirement. Don't gate the feature on hardware you may not have.
- **Default summary language:** `auto` (match the transcript's detected language). Costs nothing to default correctly even if you never hit non-English content.

**Still genuinely yours to pick — this is taste, not a default I can responsibly set for you:**
- **Whether to build the "Phase 10+" bucket at all** (calendar integration, task-manager export, collection-level rollup summaries) — these are real value-adds but real scope, and whether they're worth your time depends on how much you actually end up using recurring meetings/courses in practice. Worth deciding after you've used the v1 app for a few weeks, not before.