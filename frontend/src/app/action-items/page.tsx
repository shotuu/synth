"use client";

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { Check, Circle, Folder as FolderIcon } from 'lucide-react';
import { CONTEXT_STYLES, ContextType } from '@/components/MeetingDetails/ContextTypeSelector';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';

interface ActionItem {
  id: string;
  meeting_id: string;
  description: string;
  owner: string | null;
  due_date: string | null;
  done: boolean;
  created_at: string;
  meeting_title: string;
  folder_id: string | null;
  context_type: string;
}

/**
 * Cross-note action item view (PROJECT_BRIEF.md §12): every open todo
 * across every session, in one place, filterable by folder and status.
 */
export default function ActionItemsPage() {
  const router = useRouter();
  const { folders } = useSidebar();
  const [items, setItems] = useState<ActionItem[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [showDone, setShowDone] = useState(false);
  const [folderFilter, setFolderFilter] = useState<string | 'all'>('all');

  const load = async () => {
    setIsLoading(true);
    try {
      const result = await invoke<ActionItem[]>('api_list_action_items', {
        folderId: folderFilter === 'all' ? null : folderFilter,
        done: showDone ? null : false,
        since: null,
      });
      setItems(result);
    } catch (error) {
      console.error('Failed to load action items:', error);
      setItems([]);
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showDone, folderFilter]);

  const toggleDone = async (item: ActionItem) => {
    // Optimistic update
    setItems((prev) =>
      showDone && item.done === false
        ? prev.map((i) => (i.id === item.id ? { ...i, done: true } : i))
        : prev.filter((i) => i.id !== item.id)
    );
    try {
      await invoke('api_toggle_action_item', { actionItemId: item.id, done: !item.done });
    } catch (error) {
      console.error('Failed to toggle action item:', error);
      load(); // reconcile with server state on failure
    }
  };

  const grouped = useMemo(() => {
    const byMeeting = new Map<string, { title: string; contextType: string; items: ActionItem[] }>();
    for (const item of items) {
      if (!byMeeting.has(item.meeting_id)) {
        byMeeting.set(item.meeting_id, {
          title: item.meeting_title,
          contextType: item.context_type,
          items: [],
        });
      }
      byMeeting.get(item.meeting_id)!.items.push(item);
    }
    return Array.from(byMeeting.entries());
  }, [items]);

  return (
    <div className="p-8 max-w-3xl mx-auto">
      <h1 className="text-2xl font-bold mb-1">Action Items</h1>
      <p className="text-gray-500 text-sm mb-6">Every open todo, across every session</p>

      <div className="flex flex-wrap items-center gap-2 mb-6">
        <select
          value={folderFilter}
          onChange={(e) => setFolderFilter(e.target.value)}
          className="text-sm border border-gray-200 rounded-md px-2 py-1.5 bg-white"
        >
          <option value="all">All folders</option>
          {folders.map((f) => (
            <option key={f.id} value={f.id}>{f.icon ? `${f.icon} ` : ''}{f.name}</option>
          ))}
        </select>
        <label className="flex items-center gap-1.5 text-sm text-gray-600 px-2">
          <input type="checkbox" checked={showDone} onChange={(e) => setShowDone(e.target.checked)} />
          Show completed
        </label>
      </div>

      {isLoading ? (
        <div className="text-sm text-gray-400">Loading…</div>
      ) : grouped.length === 0 ? (
        <div className="text-sm text-gray-400 py-12 text-center">
          {showDone ? 'No action items yet.' : 'Nothing open — nice work.'}
        </div>
      ) : (
        <div className="space-y-6">
          {grouped.map(([meetingId, group]) => {
            const style = CONTEXT_STYLES[group.contextType as ContextType];
            return (
              <div key={meetingId}>
                <button
                  onClick={() => router.push(`/meeting-details?id=${meetingId}`)}
                  className="flex items-center gap-2 mb-2 text-sm font-medium text-gray-700 hover:text-blue-600"
                >
                  {style && <span className={`w-1.5 h-1.5 rounded-full ${style.dot}`} />}
                  {group.title}
                </button>
                <div className="space-y-1">
                  {group.items.map((item) => (
                    <div key={item.id} className="flex items-start gap-2.5 px-2 py-1.5 rounded-md hover:bg-gray-50 group">
                      <button
                        onClick={() => toggleDone(item)}
                        className={`mt-0.5 shrink-0 ${item.done ? 'text-emerald-500' : 'text-gray-300 hover:text-gray-400'}`}
                      >
                        {item.done ? <Check className="w-4 h-4" /> : <Circle className="w-4 h-4" />}
                      </button>
                      <div className="flex-1 min-w-0">
                        <span className={`text-sm ${item.done ? 'text-gray-400 line-through' : 'text-gray-800'}`}>
                          {item.description}
                        </span>
                        {(item.owner || item.due_date) && (
                          <div className="text-xs text-gray-400 mt-0.5">
                            {item.owner && <span>{item.owner}</span>}
                            {item.owner && item.due_date && <span> · </span>}
                            {item.due_date && <span>Due {item.due_date}</span>}
                          </div>
                        )}
                      </div>
                      {item.folder_id && (
                        <FolderIcon className="w-3 h-3 text-gray-300 shrink-0 mt-1" />
                      )}
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
