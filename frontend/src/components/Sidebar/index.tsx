'use client';

import React, { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import {
  Settings,
  Home,
  Mic,
  Search,
  X,
  Upload,
  FolderPlus,
  Folder as FolderIcon,
  Check,
  HardDrive,
  PanelLeftClose,
  PanelLeftOpen,
} from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar, UNFILED_FOLDER_ID, INTRO_CALL_ID } from './SidebarProvider';
import type { CurrentMeeting } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';
import { Dialog, DialogContent, DialogFooter, DialogTitle } from '@/components/ui/dialog';
import { VisuallyHidden } from '@/components/ui/visually-hidden';
import Info from '../Info';
import { FolderTree, TreeItem, TranscriptMatch, onEnterEscape } from './FolderTree';

const WIDTH_STORAGE_KEY = 'synth_sidebar_width';
const MIN_WIDTH = 200;
const MAX_WIDTH = 400;
const DEFAULT_WIDTH = 260;

function loadStoredWidth(): number {
  if (typeof window === 'undefined') return DEFAULT_WIDTH;
  const stored = Number(localStorage.getItem(WIDTH_STORAGE_KEY));
  return Number.isFinite(stored) && stored >= MIN_WIDTH && stored <= MAX_WIDTH ? stored : DEFAULT_WIDTH;
}

/**
 * App sidebar, rebuilt Obsidian-style for Phase 10: pinned search, one
 * primary "New session" action, quiet nav, a folder tree with indent
 * guides and hover actions, resizable width, and full-hide collapse.
 * Data flow stays in SidebarProvider; this component is presentation +
 * the folder/meeting mutations.
 */
const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const {
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    meetings,
    setMeetings,
    refetchFolders,
    refetchMeetings,
  } = useSidebar();

  const { isRecording } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const { betaFeatures } = useConfig();

  // ── Width / resize ─────────────────────────────────────────────────────
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  useEffect(() => setWidth(loadStoredWidth()), []);
  const dragState = useRef<{ startX: number; startWidth: number } | null>(null);

  const onResizeStart = (e: React.PointerEvent) => {
    dragState.current = { startX: e.clientX, startWidth: width };
    const onMove = (ev: PointerEvent) => {
      if (!dragState.current) return;
      const next = Math.min(
        MAX_WIDTH,
        Math.max(MIN_WIDTH, dragState.current.startWidth + (ev.clientX - dragState.current.startX))
      );
      setWidth(next);
    };
    const onUp = () => {
      dragState.current = null;
      setWidth((w) => {
        localStorage.setItem(WIDTH_STORAGE_KEY, String(w));
        return w;
      });
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  };

  // ── Tree expansion ─────────────────────────────────────────────────────
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(new Set([UNFILED_FOLDER_ID]));
  const seenFolderIdsRef = useRef<Set<string>>(new Set());
  const { folders } = useSidebar();
  useEffect(() => {
    // Auto-expand newly created folders once so they aren't invisible-by-default.
    const unseen = folders.filter((f) => !seenFolderIdsRef.current.has(f.id));
    if (unseen.length === 0) return;
    unseen.forEach((f) => seenFolderIdsRef.current.add(f.id));
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      unseen.forEach((f) => next.add(f.id));
      return next;
    });
  }, [folders]);

  const toggleFolder = (folderId: string) => {
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  };

  // ── Search ─────────────────────────────────────────────────────────────
  const [searchQuery, setSearchQuery] = useState('');
  const handleSearchChange = useCallback(
    async (value: string) => {
      setSearchQuery(value);
      if (!value.trim()) return;
      await searchTranscripts(value);
    },
    [searchTranscripts]
  );

  const filteredItems = useMemo((): TreeItem[] => {
    if (!searchQuery.trim()) return sidebarItems;
    const q = searchQuery.toLowerCase();
    const matchedIds = new Set(searchResults.map((r) => r.id));

    const filterNode = (item: TreeItem): TreeItem | null => {
      if (item.type === 'file') {
        return matchedIds.has(item.id) || item.title.toLowerCase().includes(q) ? item : null;
      }
      const children = (item.children ?? [])
        .map(filterNode)
        .filter((c): c is TreeItem => c !== null);
      // Keep folders that match by name, or that still contain matches.
      if (children.length > 0 || item.title.toLowerCase().includes(q)) {
        return { ...item, children };
      }
      return null;
    };

    return sidebarItems.map(filterNode).filter((i): i is TreeItem => i !== null);
  }, [sidebarItems, searchQuery, searchResults]);

  // O(1) lookup for FolderTree's per-row match rendering, built once here
  // instead of each row doing a linear find() over searchResults.
  const transcriptMatchMap = useMemo((): Map<string, TranscriptMatch> | undefined => {
    if (!searchQuery.trim()) return undefined;
    return new Map(searchResults.map((r) => [r.id, { id: r.id, matchContext: r.matchContext }]));
  }, [searchQuery, searchResults]);

  // While searching, expand everything that survived the filter.
  const effectiveExpanded = useMemo(() => {
    if (!searchQuery.trim()) return expandedFolders;
    const all = new Set<string>();
    const collect = (items: TreeItem[]) => {
      items.forEach((i) => {
        if (i.type === 'folder') {
          all.add(i.id);
          if (i.children) collect(i.children);
        }
      });
    };
    collect(filteredItems);
    return all;
  }, [searchQuery, expandedFolders, filteredItems]);

  // ── Folder mutations ───────────────────────────────────────────────────
  const [isCreatingFolder, setIsCreatingFolder] = useState(false);
  const [newFolderName, setNewFolderName] = useState('');

  const handleCreateFolder = async () => {
    const name = newFolderName.trim();
    setIsCreatingFolder(false);
    setNewFolderName('');
    if (!name) return;
    try {
      await invoke('api_create_folder', { parentFolderId: null, name, icon: null });
      await refetchFolders();
      toast.success(`Folder "${name}" created`);
    } catch (error) {
      console.error('Failed to create folder:', error);
      toast.error('Failed to create folder');
    }
  };

  const handleRenameFolder = async (folderId: string, name: string) => {
    try {
      await invoke('api_rename_folder', { folderId, name });
      await refetchFolders();
    } catch (error) {
      console.error('Failed to rename folder:', error);
      toast.error('Failed to rename folder');
    }
  };

  const handleDeleteFolder = async (folderId: string) => {
    try {
      await invoke('api_delete_folder', { folderId });
      await Promise.all([refetchFolders(), refetchMeetings()]);
      toast.success('Folder deleted - sessions inside were kept, just unfiled');
    } catch (error) {
      console.error('Failed to delete folder:', error);
      toast.error('Failed to delete folder');
    }
  };

  const handleDropIntoFolder = async (raw: string, targetFolderId: string | null) => {
    if (!raw) return;
    try {
      const dragged = JSON.parse(raw) as { id: string; kind: 'folder' | 'meeting' };
      if (dragged.id === targetFolderId) return;
      if (dragged.kind === 'meeting') {
        await invoke('api_set_meeting_folder', { meetingId: dragged.id, folderId: targetFolderId });
        await refetchMeetings();
      } else {
        await invoke('api_move_folder', {
          folderId: dragged.id,
          newParentId: targetFolderId,
          newSortOrder: 0,
        });
        await refetchFolders();
      }
    } catch (error) {
      console.error('Failed to move item:', error);
      toast.error(error instanceof Error ? error.message : 'Failed to move item');
    }
  };

  // ── Meeting mutations (rename dialog + delete confirmation) ───────────
  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({
    isOpen: false,
    itemId: null,
  });
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null }>({
    isOpen: false,
    meetingId: null,
  });
  const [editingTitle, setEditingTitle] = useState('');

  const handleDelete = async (itemId: string) => {
    try {
      await invoke('api_delete_meeting', { meetingId: itemId });
      setMeetings(meetings.filter((m: CurrentMeeting) => m.id !== itemId));
      Analytics.trackMeetingDeleted(itemId);
      toast.success('Session deleted', { description: 'All associated data has been removed' });
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: INTRO_CALL_ID, title: '+ New Call' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error('Failed to delete session', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;
    if (!meetingId) return;
    if (!newTitle) {
      toast.error('Session title cannot be empty');
      return;
    }
    try {
      await invoke('api_save_meeting_title', { meetingId, title: newTitle });
      setMeetings(meetings.map((m: CurrentMeeting) => (m.id === meetingId ? { ...m, title: newTitle } : m)));
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }
      Analytics.trackButtonClick('edit_meeting_title', 'sidebar');
      setEditModalState({ isOpen: false, meetingId: null });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error('Failed to update session title');
    }
  };

  // ── Navigation ─────────────────────────────────────────────────────────
  const openItem = (item: TreeItem) => {
    setCurrentMeeting({ id: item.id, title: item.title });
    const basePath = item.id === INTRO_CALL_ID
      ? '/'
      : `/meeting-details?id=${item.id}`;
    router.push(basePath);
  };

  // Tray integration: "open settings" now routes to the settings page (the
  // in-sidebar settings dialog this used to open was removed long ago).
  useEffect(() => {
    (window as any).openSettings = () => router.push('/settings');
    return () => {
      delete (window as any).openSettings;
    };
  }, [router]);

  const navItems = [
    { label: 'Home', icon: Home, path: '/' },
    { label: 'All Sessions', icon: FolderIcon, path: '/sessions' },
    { label: 'Action Items', icon: Check, path: '/action-items' },
    { label: 'Storage', icon: HardDrive, path: '/storage' },
  ];

  // ── Collapsed: nothing but a floating reopen affordance ───────────────
  if (isCollapsed) {
    return (
      <button
        onClick={toggleCollapse}
        className="fixed left-2 top-3 z-50 p-1.5 rounded-md text-gray-400 hover:text-gray-700 hover:bg-gray-100 transition-colors"
        title="Show sidebar"
      >
        <PanelLeftOpen className="w-4 h-4" />
      </button>
    );
  }

  return (
    <div className="relative h-screen shrink-0 flex" style={{ width }}>
      <div className="flex-1 min-w-0 h-full bg-white/50 border-r border-gray-200 flex flex-col">
        {/* Header: wordmark + hide control */}
        <div className="titlebar shrink-0 flex items-center justify-between pl-4 pr-2 pt-3 pb-1">
          <span className="text-sm font-semibold tracking-tight text-gray-900 select-none">Synth</span>
          <button
            onClick={toggleCollapse}
            className="no-drag p-1.5 rounded-md text-gray-400 hover:text-gray-700 hover:bg-gray-100 transition-colors"
            title="Hide sidebar"
          >
            <PanelLeftClose className="w-4 h-4" />
          </button>
        </div>

        {/* Search — pinned at top */}
        <div className="shrink-0 px-2.5 pt-1.5">
          <div className="relative">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-gray-400" />
            <input
              value={searchQuery}
              onChange={(e) => handleSearchChange(e.target.value)}
              placeholder="Search sessions…"
              className="w-full h-7 pl-7 pr-7 text-[13px] rounded-md bg-gray-100 border border-transparent
                         focus:border-blue-300 focus:bg-white focus:outline-none placeholder:text-gray-400 transition-colors"
            />
            {searchQuery && (
              <button
                onClick={() => handleSearchChange('')}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 p-0.5 rounded text-gray-400 hover:text-gray-600"
              >
                <X className="w-3 h-3" />
              </button>
            )}
          </div>
        </div>

        {/* New session — the one primary action */}
        <div className="shrink-0 px-2.5 pt-2">
          <button
            onClick={handleRecordingToggle}
            disabled={isRecording}
            className={`w-full h-7 flex items-center justify-center gap-1.5 text-[13px] font-medium rounded-md transition-colors
              ${
                isRecording
                  ? 'bg-red-50 text-red-500 cursor-default'
                  : 'bg-blue-600 text-white hover:bg-blue-700'
              }`}
          >
            {isRecording ? (
              <>
                <span className="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse" />
                Recording…
              </>
            ) : (
              <>
                <Mic className="w-3.5 h-3.5" />
                New session
              </>
            )}
          </button>
          {betaFeatures.importAndRetranscribe && (
            <button
              onClick={() => openImportDialog()}
              className="w-full h-7 mt-1 flex items-center justify-center gap-1.5 text-[13px] rounded-md
                         text-gray-500 hover:bg-gray-100 hover:text-gray-700 transition-colors"
            >
              <Upload className="w-3.5 h-3.5" />
              Import audio
            </button>
          )}
        </div>

        {/* Nav */}
        <nav className="shrink-0 px-2.5 pt-3 space-y-px">
          {navItems.map(({ label, icon: Icon, path }) => {
            const active = pathname === path;
            return (
              <button
                key={path}
                onClick={() => router.push(path)}
                className={`w-full flex items-center gap-2 h-7 px-1.5 rounded-r text-[13px] border-l-2 transition-colors
                  ${
                    active
                      ? 'border-blue-500 bg-blue-50 text-gray-900'
                      : 'border-transparent text-gray-500 hover:bg-gray-100 hover:text-gray-800'
                  }`}
              >
                <Icon className="w-3.5 h-3.5 shrink-0" />
                {label}
              </button>
            );
          })}
        </nav>

        {/* Folder tree */}
        <div className="flex-1 min-h-0 flex flex-col pt-3">
          <div className="shrink-0 flex items-center justify-between px-4 pb-1 group">
            <span className="text-[11px] uppercase tracking-wider text-gray-400 select-none">
              Folders
              {isSearching && <span className="ml-1.5 normal-case tracking-normal text-blue-500 animate-pulse">searching…</span>}
            </span>
            <button
              onClick={() => setIsCreatingFolder(true)}
              className="opacity-0 group-hover:opacity-100 p-0.5 rounded text-gray-400 hover:text-gray-700 transition-opacity"
              title="New folder"
            >
              <FolderPlus className="w-3.5 h-3.5" />
            </button>
          </div>

          {isCreatingFolder && (
            <div className="shrink-0 flex items-center gap-1.5 h-7 mx-2.5 px-1.5 rounded border border-dashed border-gray-300">
              <FolderIcon className="w-3.5 h-3.5 text-gray-400 shrink-0" />
              <input
                autoFocus
                value={newFolderName}
                placeholder="Folder name"
                onChange={(e) => setNewFolderName(e.target.value)}
                onBlur={handleCreateFolder}
                onKeyDown={onEnterEscape(handleCreateFolder, () => {
                  setIsCreatingFolder(false);
                  setNewFolderName('');
                })}
                className="flex-1 min-w-0 text-[13px] bg-transparent outline-none"
              />
            </div>
          )}

          <div className="flex-1 overflow-y-auto custom-scrollbar min-h-0 px-2.5 pb-4">
            <FolderTree
              items={filteredItems}
              expandedFolders={effectiveExpanded}
              onToggleFolder={toggleFolder}
              activeMeetingId={currentMeeting?.id}
              onOpenItem={openItem}
              transcriptMatches={transcriptMatchMap}
              onRenameFolder={handleRenameFolder}
              onDeleteFolder={handleDeleteFolder}
              onRenameMeeting={(meetingId, currentTitle) => {
                setEditModalState({ isOpen: true, meetingId });
                setEditingTitle(currentTitle);
              }}
              onDeleteMeeting={(meetingId) => setDeleteModalState({ isOpen: true, itemId: meetingId })}
              onDropIntoFolder={handleDropIntoFolder}
            />
          </div>
        </div>

        {/* Footer — quiet */}
        <div className="shrink-0 border-t border-gray-200 px-2.5 py-1.5 flex items-center justify-between">
          <button
            onClick={() => router.push('/settings')}
            className={`flex items-center gap-1.5 h-6 px-1.5 rounded text-xs transition-colors ${
              pathname === '/settings' ? 'text-gray-800 bg-gray-100' : 'text-gray-400 hover:text-gray-700 hover:bg-gray-100'
            }`}
          >
            <Settings className="w-3.5 h-3.5" />
            Settings
          </button>
          <div className="flex items-center gap-1 text-gray-400">
            <Info isCollapsed={false} />
            <span className="text-[10px] font-mono select-none">v0.4.0</span>
          </div>
        </div>
      </div>

      {/* Resize handle */}
      <div
        onPointerDown={onResizeStart}
        className="absolute right-0 top-0 h-full w-1 cursor-col-resize hover:bg-blue-500/40 active:bg-blue-500/60 transition-colors"
        title="Drag to resize"
      />

      {/* Delete confirmation */}
      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Are you sure you want to delete this session? This action cannot be undone."
        onConfirm={() => {
          if (deleteModalState.itemId) handleDelete(deleteModalState.itemId);
          setDeleteModalState({ isOpen: false, itemId: null });
        }}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      {/* Rename session dialog */}
      <Dialog
        open={editModalState.isOpen}
        onOpenChange={(open) => {
          if (!open) {
            setEditModalState({ isOpen: false, meetingId: null });
            setEditingTitle('');
          }
        }}
      >
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Rename session</DialogTitle>
          </VisuallyHidden>
          <div className="py-2">
            <h3 className="text-lg font-semibold mb-4">Rename session</h3>
            <input
              type="text"
              value={editingTitle}
              onChange={(e) => setEditingTitle(e.target.value)}
              onKeyDown={onEnterEscape(handleEditConfirm, () => {
                setEditModalState({ isOpen: false, meetingId: null });
                setEditingTitle('');
              })}
              className="w-full px-3 py-2 border border-gray-300 rounded-md bg-transparent focus:outline-none focus:ring-2 focus:ring-blue-500"
              placeholder="Session title"
              autoFocus
            />
          </div>
          <DialogFooter>
            <button
              onClick={() => {
                setEditModalState({ isOpen: false, meetingId: null });
                setEditingTitle('');
              }}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleEditConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;
