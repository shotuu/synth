"use client";

import { useMemo } from 'react';
import type { Block, PartialBlock } from '@blocknote/core';
import { useCreateBlockNote } from '@blocknote/react';
import { BlockNoteView } from '@blocknote/shadcn';
import '@blocknote/shadcn/style.css';
import '@blocknote/core/fonts/inter.css';
import { NotebookPen } from 'lucide-react';
import { blocksToMarkdownSafely } from '@/lib/blocknote-markdown';
import { MeetingNotesData, UseSourcesResult } from '@/hooks/meeting-details/useSources';

function parseInitialBlocks(notes: MeetingNotesData | null): PartialBlock[] | undefined {
  if (!notes?.notes_json) return undefined;
  try {
    const blocks = JSON.parse(notes.notes_json);
    return Array.isArray(blocks) && blocks.length > 0 ? blocks : undefined;
  } catch {
    return undefined;
  }
}

function NotesEditor({
  initialNotes,
  onNotesChange,
}: {
  initialNotes: MeetingNotesData | null;
  onNotesChange: (notesJson: string, notesMarkdown: string) => void;
}) {
  const initialContent = useMemo(() => parseInitialBlocks(initialNotes), [initialNotes]);

  // Default BlockNote schema already gives bullet/numbered lists, headings,
  // checkboxes, bold/italic/underline, and a "/" slash-command menu — this
  // isn't a plain textarea. Placeholder text below makes that discoverable.
  const editor = useCreateBlockNote({ initialContent });

  const handleChange = async () => {
    const blocks = editor.document as Block[];
    const result = await blocksToMarkdownSafely(editor, blocks, {
      source: 'notes-panel',
    });
    onNotesChange(JSON.stringify(blocks), result.markdown ?? '');
  };

  return (
    <BlockNoteView
      editor={editor}
      onChange={handleChange}
      theme="dark"
      data-testid="session-notes-editor"
    />
  );
}

/**
 * Your own written notes — one of three equal-height source sections
 * (PROJECT_BRIEF.md §5) alongside the transcript and attachments. Split out
 * of the old combined "Files & notes" accordion so writing has real,
 * dedicated space instead of a height-capped shared strip.
 */
export function NotesPanel({ meetingId, sources }: { meetingId: string; sources: UseSourcesResult }) {
  return (
    <div className="flex-1 min-h-0 flex flex-col bg-white border-t border-gray-200">
      <div className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium text-gray-700 shrink-0 border-b border-gray-100">
        <NotebookPen className="w-4 h-4" />
        <span>Notes</span>
        {sources.isSavingNotes && (
          <span className="text-xs text-gray-400 ml-auto">Saving…</span>
        )}
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto px-3 py-2">
        {sources.isLoading ? null : (
          // Remount the editor when the meeting or its loaded notes change.
          <NotesEditor
            key={`${meetingId}-${sources.initialNotes?.updated_at ?? 'empty'}`}
            initialNotes={sources.initialNotes}
            onNotesChange={sources.scheduleNotesSave}
          />
        )}
      </div>
    </div>
  );
}
