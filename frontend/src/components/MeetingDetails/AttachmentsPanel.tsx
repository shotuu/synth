"use client";

import { FileText, FileImage, File, Loader2, Paperclip, Plus, Trash2 } from 'lucide-react';
import { NoteAttachmentInfo, UseSourcesResult } from '@/hooks/meeting-details/useSources';

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
 * Uploaded-file attachments — one of three equal-height source sections
 * (PROJECT_BRIEF.md §5) alongside the transcript and notes, all feeding the
 * context assembler. Previously bundled with notes under a single
 * height-capped "Files & notes" accordion; split out so each source gets
 * real space instead of one dominating.
 */
export function AttachmentsPanel({ sources }: { sources: UseSourcesResult }) {
  return (
    <div className="flex-1 min-h-0 flex flex-col bg-white border-t border-gray-200">
      <div className="flex items-center gap-2 px-4 py-2.5 text-sm font-medium text-gray-700 shrink-0 border-b border-gray-100">
        <Paperclip className="w-4 h-4" />
        <span>Files</span>
        {sources.attachments.length > 0 && (
          <span className="text-xs text-gray-400">({sources.attachments.length})</span>
        )}
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto px-2 py-2">
        {sources.isLoading ? (
          <div className="flex items-center gap-2 px-3 py-2 text-sm text-gray-400">
            <Loader2 className="w-4 h-4 animate-spin" /> Loading…
          </div>
        ) : (
          <>
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
          </>
        )}
      </div>
    </div>
  );
}
