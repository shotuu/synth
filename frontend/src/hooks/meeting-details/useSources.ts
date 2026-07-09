import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

export interface NoteAttachmentInfo {
  id: string;
  meeting_id: string;
  file_name: string;
  file_type: string;
  storage_path: string;
  has_extracted_text: boolean;
  uploaded_at: string;
}

export interface MeetingNotesData {
  meeting_id: string;
  notes_markdown: string | null;
  notes_json: string | null;
  created_at: string;
  updated_at: string;
}

const NOTES_AUTOSAVE_DELAY_MS = 1200;

export interface UseSourcesResult {
  attachments: NoteAttachmentInfo[];
  initialNotes: MeetingNotesData | null;
  isLoading: boolean;
  isAttaching: boolean;
  isSavingNotes: boolean;
  attachFiles: () => Promise<void>;
  deleteAttachment: (id: string) => Promise<void>;
  scheduleNotesSave: (notesJson: string, notesMarkdown: string) => void;
}

/**
 * Loads and mutates a session's sources: file attachments and the user's
 * own written notes. Notes saves are debounced; attachments update
 * optimistically after each backend call.
 */
export function useSources(meetingId: string): UseSourcesResult {
  const [attachments, setAttachments] = useState<NoteAttachmentInfo[]>([]);
  const [initialNotes, setInitialNotes] = useState<MeetingNotesData | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isAttaching, setIsAttaching] = useState(false);
  const [isSavingNotes, setIsSavingNotes] = useState(false);

  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Guards against a stale debounce firing after the meeting changed
  const meetingIdRef = useRef(meetingId);
  meetingIdRef.current = meetingId;

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setIsLoading(true);
      try {
        const [attachmentList, notes] = await Promise.all([
          invoke<NoteAttachmentInfo[]>('api_list_attachments', { meetingId }),
          invoke<MeetingNotesData | null>('api_get_meeting_notes', { meetingId }),
        ]);
        if (!cancelled) {
          setAttachments(attachmentList);
          setInitialNotes(notes);
        }
      } catch (error) {
        console.error('Failed to load session sources:', error);
        if (!cancelled) {
          setAttachments([]);
          setInitialNotes(null);
        }
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    };
  }, [meetingId]);

  /** Debounced notes save; called by the editor on every change. */
  const scheduleNotesSave = useCallback(
    (notesJson: string, notesMarkdown: string) => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      const targetMeetingId = meetingIdRef.current;

      saveTimerRef.current = setTimeout(async () => {
        setIsSavingNotes(true);
        try {
          await invoke('api_save_meeting_notes', {
            meetingId: targetMeetingId,
            notesMarkdown,
            notesJson,
          });
        } catch (error) {
          console.error('Failed to save notes:', error);
          toast.error('Failed to save your notes');
        } finally {
          setIsSavingNotes(false);
        }
      }, NOTES_AUTOSAVE_DELAY_MS);
    },
    []
  );

  /** Open the OS file picker and attach the chosen files. */
  const attachFiles = useCallback(async () => {
    setIsAttaching(true);
    try {
      const added = await invoke<Array<{ id: string }>>('api_attach_files', {
        meetingId,
      });
      if (added.length > 0) {
        const refreshed = await invoke<NoteAttachmentInfo[]>('api_list_attachments', {
          meetingId,
        });
        setAttachments(refreshed);
        toast.success(
          added.length === 1 ? 'File added to this session' : `${added.length} files added to this session`
        );
      }
    } catch (error) {
      console.error('Failed to attach files:', error);
      toast.error('Failed to add files');
    } finally {
      setIsAttaching(false);
    }
  }, [meetingId]);

  const deleteAttachment = useCallback(
    async (attachmentId: string) => {
      try {
        await invoke<boolean>('api_delete_attachment', { attachmentId });
        setAttachments((prev) => prev.filter((a) => a.id !== attachmentId));
        toast.success('File removed');
      } catch (error) {
        console.error('Failed to delete attachment:', error);
        toast.error('Failed to remove file');
      }
    },
    []
  );

  return {
    attachments,
    initialNotes,
    isLoading,
    isAttaching,
    isSavingNotes,
    attachFiles,
    deleteAttachment,
    scheduleNotesSave,
  };
}
