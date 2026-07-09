"use client";

import { useMemo, useState } from 'react';
import type { Block, PartialBlock } from '@blocknote/core';
import { useCreateBlockNote } from '@blocknote/react';
import { BlockNoteView } from '@blocknote/shadcn';
import '@blocknote/shadcn/style.css';
import '@blocknote/core/fonts/inter.css';
import {
  ChevronDown,
  ChevronRight,
  FileText,
  FileImage,
  File,
  Loader2,
  Paperclip,
  Plus,
  Trash2,
} from 'lucide-react';
import { blocksToMarkdownSafely } from '@/lib/blocknote-markdown';
import { useSources, NoteAttachmentInfo, MeetingNotesData } from '@/hooks/meeting-details/useSources';

function attachmentIcon(fileType: string) {
  switch (fileType) {
    case 'image':
      return <FileImage className="w-4 h-4 text-gray-400 shrink-0" />;
    case 'pdf':
    case 'docx':
    case 'pptx':
    case 'txt':
    case 'md':
      return <FileText className="w-4 h-4 text-gray-400 shrink-0" />;
    default:
      return <File className="w-4 h-4 text-gray-400 shrink-0" />;
  }
}

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

  const editor = useCreateBlockNote({ initialContent });

  const handleChange = async () => {
    const blocks = editor.document as Block[];
    const result = await blocksToMarkdownSafely(editor, blocks, {
      source: 'sources-panel-notes',
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

function AttachmentRow({
  attachment,
  onDelete,
}: {
  attachment: NoteAttachmentInfo;
  onDelete: (id: string) => void;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5 rounded-md hover:bg-gray-50 group">
      {attachmentIcon(attachment.file_type)}
      <span className="text-sm text-gray-700 truncate flex-1" title={attachment.file_name}>
        {attachment.file_name}
      </span>
      {!attachment.has_extracted_text && attachment.file_type !== 'image' && (
        <span className="text-xs text-gray-400 shrink-0" title="No text could be extracted from this file">
          no text
        </span>
      )}
      <button
        onClick={() => onDelete(attachment.id)}
        className="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-red-500 transition-opacity shrink-0"
        title="Remove file"
      >
        <Trash2 className="w-3.5 h-3.5" />
      </button>
    </div>
  );
}

/**
 * Session sources below the transcript: file attachments and the user's own
 * notes. Together with the transcript these feed the context assembler
 * (PROJECT_BRIEF.md §5).
 */
export function SourcesPanel({ meetingId }: { meetingId: string }) {
  const sources = useSources(meetingId);
  const [isExpanded, setIsExpanded] = useState(true);

  return (
    <div className="border-t border-gray-200 bg-white flex flex-col min-h-0 max-h-[45%]">
      <button
        onClick={() => setIsExpanded((v) => !v)}
        className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium text-gray-700 hover:bg-gray-50 shrink-0"
      >
        {isExpanded ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
        <Paperclip className="w-4 h-4" />
        <span>Files & notes</span>
        {sources.attachments.length > 0 && (
          <span className="text-xs text-gray-400">({sources.attachments.length})</span>
        )}
        {sources.isSavingNotes && (
          <span className="text-xs text-gray-400 ml-auto">Saving…</span>
        )}
      </button>

      {isExpanded && (
        <div className="overflow-y-auto px-2 pb-3 flex-1 min-h-0">
          {sources.isLoading ? (
            <div className="flex items-center gap-2 px-3 py-2 text-sm text-gray-400">
              <Loader2 className="w-4 h-4 animate-spin" /> Loading…
            </div>
          ) : (
            <>
              <div className="mb-2">
                {sources.attachments.map((a) => (
                  <AttachmentRow key={a.id} attachment={a} onDelete={sources.deleteAttachment} />
                ))}
                <button
                  onClick={sources.attachFiles}
                  disabled={sources.isAttaching}
                  className="flex items-center gap-2 px-3 py-1.5 text-sm text-gray-500 hover:text-gray-700 hover:bg-gray-50 rounded-md w-full disabled:opacity-50"
                >
                  {sources.isAttaching ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : (
                    <Plus className="w-4 h-4" />
                  )}
                  Add files to this session
                </button>
              </div>

              <div className="px-1">
                <div className="text-xs font-medium text-gray-400 uppercase tracking-wide px-2 pb-1">
                  Your notes
                </div>
                {/* Remount the editor when the meeting or its loaded notes change */}
                <NotesEditor
                  key={`${meetingId}-${sources.initialNotes?.updated_at ?? 'empty'}`}
                  initialNotes={sources.initialNotes}
                  onNotesChange={sources.scheduleNotesSave}
                />
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
