"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Folder as FolderIcon, Download, Loader2 } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

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

  useEffect(() => {
    setCurrentFolderId(folderId ?? null);
  }, [folderId, meetingId]);

  const handleFolderChange = async (newFolderId: string) => {
    const resolved = newFolderId === '' ? null : newFolderId;
    setCurrentFolderId(resolved);
    try {
      await invoke('api_set_meeting_folder', { meetingId, folderId: resolved });
      await refetchMeetings();
    } catch (error) {
      console.error('Failed to move session:', error);
      toast.error('Failed to move session');
    }
  };

  const handleExport = async () => {
    setIsExporting(true);
    try {
      const path = await invoke<string | null>('api_export_session_html', { meetingId });
      if (path) {
        toast.success('Session exported', { description: path });
      }
    } catch (error) {
      console.error('Failed to export session:', error);
      toast.error('Failed to export session');
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="flex items-center gap-1.5">
      <div className="relative">
        <FolderIcon className="w-3.5 h-3.5 text-gray-400 absolute left-2 top-1/2 -translate-y-1/2 pointer-events-none" />
        <select
          value={currentFolderId ?? ''}
          onChange={(e) => handleFolderChange(e.target.value)}
          className="text-xs border border-gray-200 rounded-full pl-6 pr-2 py-1 bg-white appearance-none cursor-pointer hover:bg-gray-50"
          title="Move to folder"
        >
          <option value="">No folder</option>
          {folders.map((f) => (
            <option key={f.id} value={f.id}>{f.icon ? `${f.icon} ` : ''}{f.name}</option>
          ))}
        </select>
      </div>
      <button
        onClick={handleExport}
        disabled={isExporting}
        className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border border-gray-200 text-xs font-medium text-gray-600 hover:bg-gray-50 disabled:opacity-50"
        title="Export this session as a shareable HTML page"
      >
        {isExporting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Download className="w-3.5 h-3.5" />}
        Export
      </button>
    </div>
  );
}
