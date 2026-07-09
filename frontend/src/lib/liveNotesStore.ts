/**
 * Draft store for notes typed *during* a recording (Phase 10 live-recording
 * view). A meeting row — and therefore a meeting_id to save meeting_notes
 * against — only exists once the stop flow's save completes, so live notes
 * buffer here and are flushed into meeting_notes by useRecordingStop as soon
 * as the id arrives. The review view then reads the exact same record.
 *
 * localStorage (not IndexedDB) on purpose: notes are keystroke-sized text,
 * written synchronously on every editor change, and must survive a crash
 * mid-recording. If the app dies before the flush, the draft is still here —
 * the next live session offers it as the editor's initial content instead of
 * silently discarding it.
 */

const STORAGE_KEY = 'synth_live_notes_draft';

export interface LiveNotesDraft {
  notesJson: string;
  notesMarkdown: string;
  updatedAt: string;
}

export function saveLiveNotesDraft(notesJson: string, notesMarkdown: string): void {
  try {
    const draft: LiveNotesDraft = {
      notesJson,
      notesMarkdown,
      updatedAt: new Date().toISOString(),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(draft));
  } catch (error) {
    // Quota/serialization failures shouldn't interrupt typing.
    console.warn('[liveNotes] Failed to persist draft:', error);
  }
}

export function loadLiveNotesDraft(): LiveNotesDraft | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const draft = JSON.parse(raw) as LiveNotesDraft;
    return draft.notesJson ? draft : null;
  } catch {
    return null;
  }
}

/** True when the draft has any real content (not just an empty document). */
export function draftHasContent(draft: LiveNotesDraft | null): draft is LiveNotesDraft {
  if (!draft) return false;
  if (draft.notesMarkdown.trim().length > 0) return true;
  // Markdown conversion can lossily drop some block types; fall back to
  // checking the block JSON for any non-empty content field.
  try {
    const blocks = JSON.parse(draft.notesJson) as Array<{ content?: unknown[] }>;
    return blocks.some((b) => Array.isArray(b.content) && b.content.length > 0);
  } catch {
    return false;
  }
}

export function clearLiveNotesDraft(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
}
