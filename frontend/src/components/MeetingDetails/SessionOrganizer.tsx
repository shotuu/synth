"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Folder as FolderIcon, Download, Loader2, ChevronDown } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from '@/components/ui/select';
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';

/** Sentinel for "not in any folder" — Radix Select items can't have value="". */
const NO_FOLDER = '__none__';

/**
 * Folder assignment + static-page export for the current session. Folder
 * assignment mirrors what drag-and-drop onto the sidebar tree does, for
 * anyone who'd rather pick from a list than drag; export is the practical
 * stand-in for a share link (PROJECT_BRIEF.md §12 — see organization/export.rs
 * for why: this app has no server to host a live link on).
 */
export function SessionOrganizer({ meetingId, folderId }: { meetingId: string; folderId?: string | null }) {
  const { folders, refetchMeetings } = useSidebar();
  const [currentFolderId, setCurrentFolderId] = useState<string | null>(folderId ?? null);
  const [isExporting, setIsExporting] = useState(false);
  const [aiCleaned, setAiCleaned] = useState(false);

  useEffect(() => {
    setCurrentFolderId(folderId ?? null);
  }, [folderId, meetingId]);

  const handleFolderChange = async (newFolderId: string) => {
    const resolved = newFolderId === NO_FOLDER ? null : newFolderId;
    setCurrentFolderId(resolved);
    try {
      await invoke('api_set_meeting_folder', { meetingId, folderId: resolved });
      await refetchMeetings();
    } catch (error) {
      console.error('Failed to move session:', error);
      toast.error('Failed to move session');
    }
  };

  const EXPORT_COMMANDS: Record<'html' | 'pdf' | 'docx', string> = {
    html: 'api_export_session_html',
    pdf: 'api_export_session_pdf',
    docx: 'api_export_session_docx',
  };

  const handleExport = async (format: keyof typeof EXPORT_COMMANDS) => {
    setIsExporting(true);
    try {
      const path = await invoke<string | null>(EXPORT_COMMANDS[format], { meetingId, aiCleaned });
      if (path) {
        toast.success('Session exported', { description: path });
      }
    } catch (error) {
      console.error('Failed to export session:', error);
      toast.error(aiCleaned ? 'AI cleanup failed' : 'Failed to export session', {
        description: aiCleaned ? String(error) : undefined,
      });
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="flex items-center gap-1.5">
      <Select value={currentFolderId ?? NO_FOLDER} onValueChange={handleFolderChange}>
        <SelectTrigger
          className="h-auto w-auto gap-1 rounded-full border-gray-200 bg-white px-2.5 py-1 text-xs hover:bg-gray-50 [&>svg]:w-3 [&>svg]:h-3"
          title="Move to folder"
        >
          <FolderIcon className="w-3.5 h-3.5 text-gray-400" />
          <SelectValue placeholder="No folder" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NO_FOLDER} className="text-xs">No folder</SelectItem>
          {folders.map((f) => (
            <SelectItem key={f.id} value={f.id} className="text-xs">
              {f.icon ? `${f.icon} ` : ''}{f.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            disabled={isExporting}
            className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border border-gray-200 text-xs font-medium text-gray-600 hover:bg-gray-50 disabled:opacity-50"
            title="Export this session to send or use elsewhere"
          >
            {isExporting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Download className="w-3.5 h-3.5" />}
            {isExporting && aiCleaned ? 'Cleaning up…' : 'Export'}
            <ChevronDown className="w-3 h-3 opacity-60" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[190px]">
          <div className="px-2 py-1.5">
            <div className="flex rounded-md border border-gray-200 p-0.5 text-[11px] font-medium">
              <button
                type="button"
                onClick={() => setAiCleaned(false)}
                className={`flex-1 rounded px-2 py-1 transition-colors ${!aiCleaned ? 'bg-gray-900 text-gray-50' : 'text-gray-500 hover:text-gray-700'}`}
              >
                Raw
              </button>
              <button
                type="button"
                onClick={() => setAiCleaned(true)}
                className={`flex-1 rounded px-2 py-1 transition-colors ${aiCleaned ? 'bg-gray-900 text-gray-50' : 'text-gray-500 hover:text-gray-700'}`}
              >
                AI-cleaned
              </button>
            </div>
            {aiCleaned && (
              <p className="mt-1 text-[10px] leading-snug text-gray-400">
                Removes filler words and reflows into paragraphs, per speaker. Uses your configured AI provider and may take a while.
              </p>
            )}
          </div>
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => handleExport('html')} className="flex-col items-start">
            <span className="text-xs font-medium">Shareable page (.html)</span>
            <span className="text-[11px] text-gray-400">Opens in any browser, no app needed</span>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => handleExport('pdf')}>
            <span className="text-xs font-medium">PDF (.pdf)</span>
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => handleExport('docx')}>
            <span className="text-xs font-medium">Word Document (.docx)</span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
