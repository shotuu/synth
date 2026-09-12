"use client";

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { formatDistanceToNow } from 'date-fns';
import {
  HardDrive,
  Trash2,
  Archive,
  AlertTriangle,
  Loader2,
  Folder as FolderIcon,
  ArrowUpDown,
  Sparkles,
} from 'lucide-react';
import { CONTEXT_STYLES, ContextType } from '@/components/MeetingDetails/ContextTypeSelector';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { formatBytes } from '@/lib/format';

interface StorageStats {
  audio_bytes: number;
  attachments_bytes: number;
  database_bytes: number;
  session_count: number;
  sessions_with_retained_audio: number;
}

interface SessionAudioRow {
  meeting_id: string;
  title: string;
  context_type: string;
  folder_id: string | null;
  created_at: string;
  updated_at: string;
  current_size_bytes: number | null;
  bitrate_kbps: number | null;
  retained: boolean;
  last_compressed_at: string | null;
  storage_path: string | null;
}

type SortKey = 'size' | 'date' | 'title';

const SUGGESTED_CLEANUP_DAYS = 30;

export default function StorageManagerPage() {
  const { folders } = useSidebar();
  const [stats, setStats] = useState<StorageStats | null>(null);
  const [sessions, setSessions] = useState<SessionAudioRow[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [sortKey, setSortKey] = useState<SortKey>('size');
  const [sortDesc, setSortDesc] = useState(true);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [showSuggestedOnly, setShowSuggestedOnly] = useState(false);
  const [suggestedIds, setSuggestedIds] = useState<Set<string>>(new Set());
  const [isBusy, setIsBusy] = useState(false);
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);

  const load = async () => {
    setIsLoading(true);
    try {
      const [statsResult, sessionsResult, suggestedResult] = await Promise.all([
        invoke<StorageStats>('api_get_storage_stats'),
        invoke<SessionAudioRow[]>('api_list_session_audio'),
        invoke<SessionAudioRow[]>('api_suggested_cleanup', { olderThanDays: SUGGESTED_CLEANUP_DAYS }),
      ]);
      setStats(statsResult);
      setSessions(sessionsResult);
      setSuggestedIds(new Set(suggestedResult.map((s) => s.meeting_id)));
    } catch (error) {
      console.error('Failed to load storage data:', error);
      toast.error('Failed to load storage data');
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const folderById = useMemo(() => {
    const map = new Map(folders.map((f) => [f.id, f]));
    return map;
  }, [folders]);

  const visibleSessions = useMemo(() => {
    const base = showSuggestedOnly ? sessions.filter((s) => suggestedIds.has(s.meeting_id)) : sessions;
    const sorted = [...base].sort((a, b) => {
      let cmp = 0;
      if (sortKey === 'size') cmp = (a.current_size_bytes ?? 0) - (b.current_size_bytes ?? 0);
      else if (sortKey === 'date') cmp = a.created_at.localeCompare(b.created_at);
      else cmp = a.title.localeCompare(b.title);
      return sortDesc ? -cmp : cmp;
    });
    return sorted;
  }, [sessions, showSuggestedOnly, suggestedIds, sortKey, sortDesc]);

  const toggleSort = (key: SortKey) => {
    if (sortKey === key) setSortDesc((d) => !d);
    else { setSortKey(key); setSortDesc(true); }
  };

  const toggleSelected = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const selectAllVisible = () => {
    setSelected(new Set(visibleSessions.map((s) => s.meeting_id)));
  };

  const clearSelection = () => setSelected(new Set());

  const runAction = async (
    command: string,
    successMessage: (n: number) => string
  ) => {
    if (selected.size === 0) return;
    setIsBusy(true);
    try {
      const count = await invoke<number>(command, { meetingIds: Array.from(selected) });
      toast.success(successMessage(count));
      clearSelection();
      setConfirmDeleteAll(false);
      await load();
    } catch (error) {
      console.error(`${command} failed:`, error);
      toast.error(`Action failed: ${error}`);
    } finally {
      setIsBusy(false);
    }
  };

  const totalBytes = stats ? stats.audio_bytes + stats.attachments_bytes + stats.database_bytes : 0;

  return (
    <div className="p-8 max-w-4xl mx-auto">
      <h1 className="text-2xl font-bold mb-1 flex items-center gap-2">
        <HardDrive className="w-5 h-5" /> Storage Manager
      </h1>
      <p className="text-gray-500 text-sm mb-6">See what's using space, and clean it up without leaving the app</p>

      {/* Aggregate stats */}
      {stats && (
        <div className="border border-gray-100 rounded-lg p-4 mb-6">
          <div className="flex items-baseline justify-between mb-3">
            <span className="text-sm text-gray-500">Total app storage</span>
            <span className="text-xl font-semibold">{formatBytes(totalBytes)}</span>
          </div>
          <div className="flex h-2 rounded-full overflow-hidden bg-gray-100 mb-3">
            {totalBytes > 0 && (
              <>
                <div className="bg-blue-400" style={{ width: `${(stats.audio_bytes / totalBytes) * 100}%` }} />
                <div className="bg-emerald-400" style={{ width: `${(stats.attachments_bytes / totalBytes) * 100}%` }} />
                <div className="bg-gray-400" style={{ width: `${(stats.database_bytes / totalBytes) * 100}%` }} />
              </>
            )}
          </div>
          <div className="flex flex-wrap gap-x-6 gap-y-1 text-xs text-gray-500">
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-blue-400" /> Audio - {formatBytes(stats.audio_bytes)}</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-emerald-400" /> Attachments - {formatBytes(stats.attachments_bytes)}</span>
            <span className="flex items-center gap-1.5"><span className="w-2 h-2 rounded-full bg-gray-400" /> Database - {formatBytes(stats.database_bytes)}</span>
            <span className="ml-auto">{stats.sessions_with_retained_audio} of {stats.session_count} sessions retain audio</span>
          </div>
        </div>
      )}

      {/* Suggested cleanup toggle */}
      {suggestedIds.size > 0 && (
        <button
          onClick={() => setShowSuggestedOnly((v) => !v)}
          className={`flex items-center gap-2 mb-4 px-3 py-2 rounded-md text-sm w-full text-left border ${
            showSuggestedOnly ? 'bg-amber-50 border-amber-200 text-amber-800' : 'border-gray-200 text-gray-600 hover:bg-gray-50'
          }`}
        >
          <Sparkles className="w-4 h-4 shrink-0" />
          {suggestedIds.size} session{suggestedIds.size === 1 ? '' : 's'} retain audio untouched for {SUGGESTED_CLEANUP_DAYS}+ days
          <span className="ml-auto text-xs underline">{showSuggestedOnly ? 'Show all' : 'Show these'}</span>
        </button>
      )}

      {/* Bulk action bar */}
      <div className="flex items-center gap-2 mb-3 flex-wrap">
        <button onClick={selectAllVisible} className="text-xs text-gray-500 hover:text-gray-700 px-2 py-1">
          Select all ({visibleSessions.length})
        </button>
        {selected.size > 0 && (
          <>
            <button onClick={clearSelection} className="text-xs text-gray-400 hover:text-gray-600 px-2 py-1">
              Clear ({selected.size} selected)
            </button>
            <div className="ml-auto flex items-center gap-2">
              <button
                disabled={isBusy}
                onClick={() => runAction('api_delete_session_audio', (n) => `Deleted audio for ${n} session${n === 1 ? '' : 's'}, transcripts kept`)}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-gray-200 text-gray-700 hover:bg-gray-50 disabled:opacity-50"
              >
                <Trash2 className="w-3.5 h-3.5" /> Delete audio, keep transcript
              </button>
              <button
                disabled={isBusy}
                onClick={() => runAction('api_compress_session_audio', (n) => `Compressed audio for ${n} session${n === 1 ? '' : 's'}`)}
                className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-gray-200 text-gray-700 hover:bg-gray-50 disabled:opacity-50"
              >
                <Archive className="w-3.5 h-3.5" /> Compress audio
              </button>
              {confirmDeleteAll ? (
                <button
                  disabled={isBusy}
                  onClick={() => runAction('api_delete_sessions', (n) => `Deleted ${n} session${n === 1 ? '' : 's'} entirely`)}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md bg-red-600 text-white hover:bg-red-700 disabled:opacity-50"
                >
                  <AlertTriangle className="w-3.5 h-3.5" /> Confirm delete {selected.size} session{selected.size === 1 ? '' : 's'}
                </button>
              ) : (
                <button
                  disabled={isBusy}
                  onClick={() => setConfirmDeleteAll(true)}
                  className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-red-200 text-red-600 hover:bg-red-50 disabled:opacity-50"
                >
                  <Trash2 className="w-3.5 h-3.5" /> Delete entire session
                </button>
              )}
              {isBusy && <Loader2 className="w-4 h-4 animate-spin text-gray-400" />}
            </div>
          </>
        )}
      </div>

      {/* Session table */}
      {isLoading ? (
        <div className="text-sm text-gray-400">Loading…</div>
      ) : visibleSessions.length === 0 ? (
        <div className="text-sm text-gray-400 py-12 text-center">
          {showSuggestedOnly ? 'Nothing needs cleanup right now.' : 'No sessions with tracked audio yet.'}
        </div>
      ) : (
        <div className="border border-gray-100 rounded-lg overflow-hidden">
          <div className="flex items-center gap-3 px-3 py-2 bg-gray-50 text-xs font-medium text-gray-500 border-b border-gray-100">
            <span className="w-4" />
            <button onClick={() => toggleSort('title')} className="flex-1 flex items-center gap-1 hover:text-gray-700 text-left">
              Session <ArrowUpDown className="w-3 h-3" />
            </button>
            <span className="w-24 shrink-0">Type</span>
            <span className="w-24 shrink-0">Folder</span>
            <button onClick={() => toggleSort('date')} className="w-24 shrink-0 flex items-center gap-1 hover:text-gray-700">
              Date <ArrowUpDown className="w-3 h-3" />
            </button>
            <button onClick={() => toggleSort('size')} className="w-20 shrink-0 flex items-center gap-1 justify-end hover:text-gray-700">
              Size <ArrowUpDown className="w-3 h-3" />
            </button>
            <span className="w-16 shrink-0 text-right">Status</span>
          </div>
          <div className="divide-y divide-gray-50">
            {visibleSessions.map((session) => {
              const style = CONTEXT_STYLES[session.context_type as ContextType];
              const folder = session.folder_id ? folderById.get(session.folder_id) : null;
              const isSuggested = suggestedIds.has(session.meeting_id);
              return (
                <div key={session.meeting_id} className={`flex items-center gap-3 px-3 py-2 text-sm ${selected.has(session.meeting_id) ? 'bg-blue-50' : 'hover:bg-gray-50'}`}>
                  <input
                    type="checkbox"
                    checked={selected.has(session.meeting_id)}
                    onChange={() => toggleSelected(session.meeting_id)}
                    className="w-4 h-4"
                  />
                  <span className="flex-1 truncate flex items-center gap-1.5">
                    {session.title}
                    {isSuggested && <Sparkles className="w-3 h-3 text-amber-400 shrink-0" aria-label="Suggested cleanup" />}
                  </span>
                  <span className="w-24 shrink-0">
                    {style && (
                      <span className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full text-[11px] ${style.chip}`}>
                        <span className={`w-1.5 h-1.5 rounded-full ${style.dot}`} />
                        {style.label}
                      </span>
                    )}
                  </span>
                  <span className="w-24 shrink-0 text-xs text-gray-400 truncate flex items-center gap-1">
                    {folder && <><FolderIcon className="w-3 h-3 shrink-0" />{folder.name}</>}
                  </span>
                  <span className="w-24 shrink-0 text-xs text-gray-400">
                    {formatDistanceToNow(new Date(session.created_at), { addSuffix: true })}
                  </span>
                  <span className="w-20 shrink-0 text-right text-xs text-gray-600 font-mono">
                    {session.current_size_bytes ? formatBytes(session.current_size_bytes) : '-'}
                  </span>
                  <span className="w-16 shrink-0 text-right text-[11px]">
                    {session.retained ? (
                      <span className="text-gray-500">retained</span>
                    ) : (
                      <span className="text-gray-300">deleted</span>
                    )}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
