"use client";

import { Suspense, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { formatDistanceToNow } from 'date-fns';
import { Search, Folder as FolderIcon, Tag as TagIcon, X } from 'lucide-react';
import { CONTEXT_STYLES, CONTEXT_ORDER, ContextType } from '@/components/MeetingDetails/ContextTypeSelector';
import { useSidebar, OrgFolder } from '@/components/Sidebar/SidebarProvider';
import { EmptyState } from '@/components/EmptyState';

interface SessionRow {
  id: string;
  title: string;
  context_type?: string;
  folder_id?: string | null;
  tags?: string | null; // JSON array string
  created_at?: string;
}

function parseTags(tags?: string | null): string[] {
  if (!tags) return [];
  try {
    const parsed = JSON.parse(tags);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

/**
 * Filterable list of every session (PROJECT_BRIEF.md §9 Phase 5) — filter
 * by context type, folder, and tag; sort by recency. The folder tree in the
 * sidebar is for browsing; this page is for filtering/scanning everything.
 */
export default function SessionsPage() {
  return (
    <Suspense fallback={null}>
      <SessionsPageInner />
    </Suspense>
  );
}

function SessionsPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const { folders } = useSidebar();
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [query, setQuery] = useState('');
  const [contextFilter, setContextFilter] = useState<ContextType | 'all'>('all');
  const [folderFilter, setFolderFilter] = useState<string | 'all'>(searchParams.get('folder') ?? 'all');
  const [tagFilter, setTagFilter] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<SessionRow[]>('api_get_meetings')
      .then((rows) => { if (!cancelled) setSessions(rows); })
      .catch((error) => {
        console.error('Failed to load sessions:', error);
        if (!cancelled) setSessions([]);
      })
      .finally(() => { if (!cancelled) setIsLoading(false); });
    return () => { cancelled = true; };
  }, []);

  const folderById = useMemo(() => {
    const map = new Map<string, OrgFolder>();
    folders.forEach((f) => map.set(f.id, f));
    return map;
  }, [folders]);

  const allTags = useMemo(() => {
    const tags = new Set<string>();
    sessions.forEach((s) => parseTags(s.tags).forEach((t) => tags.add(t)));
    return Array.from(tags).sort();
  }, [sessions]);

  const filtered = useMemo(() => {
    return sessions
      .filter((s) => {
        if (query.trim() && !s.title.toLowerCase().includes(query.trim().toLowerCase())) return false;
        if (contextFilter !== 'all' && s.context_type !== contextFilter) return false;
        if (folderFilter !== 'all') {
          if (folderFilter === 'none' ? !!s.folder_id : s.folder_id !== folderFilter) return false;
        }
        if (tagFilter && !parseTags(s.tags).includes(tagFilter)) return false;
        return true;
      })
      .sort((a, b) => (b.created_at ?? '').localeCompare(a.created_at ?? ''));
  }, [sessions, query, contextFilter, folderFilter, tagFilter]);

  const hasActiveFilters = query.trim() !== '' || contextFilter !== 'all' || folderFilter !== 'all' || !!tagFilter;

  return (
    <div className="p-8 max-w-4xl mx-auto">
      <h1 className="text-2xl font-bold mb-1">All Sessions</h1>
      <p className="text-gray-500 text-sm mb-6">{sessions.length} session{sessions.length === 1 ? '' : 's'} total</p>

      <div className="flex flex-wrap items-center gap-2 mb-6">
        <div className="relative flex-1 min-w-[200px]">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search by title..."
            className="w-full pl-8 pr-3 py-1.5 text-sm border border-gray-200 rounded-md focus:outline-none focus:ring-1 focus:ring-blue-400"
          />
        </div>

        <select
          value={contextFilter}
          onChange={(e) => setContextFilter(e.target.value as ContextType | 'all')}
          className="text-sm border border-gray-200 rounded-md px-2 py-1.5 bg-white"
        >
          <option value="all">All types</option>
          {CONTEXT_ORDER.map((ct) => (
            <option key={ct} value={ct}>{CONTEXT_STYLES[ct].label}</option>
          ))}
        </select>

        <select
          value={folderFilter}
          onChange={(e) => setFolderFilter(e.target.value)}
          className="text-sm border border-gray-200 rounded-md px-2 py-1.5 bg-white"
        >
          <option value="all">All folders</option>
          <option value="none">No folder</option>
          {folders.map((f) => (
            <option key={f.id} value={f.id}>{f.icon ? `${f.icon} ` : ''}{f.name}</option>
          ))}
        </select>

        {hasActiveFilters && (
          <button
            onClick={() => { setQuery(''); setContextFilter('all'); setFolderFilter('all'); setTagFilter(null); }}
            className="flex items-center gap-1 text-xs text-gray-400 hover:text-gray-600 px-2 py-1.5"
          >
            <X className="w-3.5 h-3.5" /> Clear filters
          </button>
        )}
      </div>

      {allTags.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 mb-6">
          <TagIcon className="w-3.5 h-3.5 text-gray-400" />
          {allTags.map((tag) => (
            <button
              key={tag}
              onClick={() => setTagFilter((prev) => (prev === tag ? null : tag))}
              className={`px-2 py-0.5 rounded-full text-xs border ${
                tagFilter === tag
                  ? 'bg-gray-800 text-white border-gray-800'
                  : 'bg-gray-50 text-gray-600 border-gray-200 hover:border-gray-300'
              }`}
            >
              {tag}
            </button>
          ))}
        </div>
      )}

      {isLoading ? (
        <div className="text-sm text-gray-400">Loading sessions…</div>
      ) : filtered.length === 0 ? (
        sessions.length === 0 ? (
          <EmptyState
            icon={FolderIcon}
            title="No sessions yet"
            description="Record a meeting, lecture, or chat and it'll show up here — organized by folder, type, and tag."
          />
        ) : (
          <EmptyState
            icon={Search}
            title="No sessions match these filters"
            description="Try clearing a filter or searching for something else."
            action={
              <button
                onClick={() => { setQuery(''); setContextFilter('all'); setFolderFilter('all'); setTagFilter(null); }}
                className="text-xs font-medium text-blue-600 hover:text-blue-700"
              >
                Clear filters
              </button>
            }
          />
        )
      ) : (
        <div className="divide-y divide-gray-100 border border-gray-100 rounded-lg overflow-hidden">
          {filtered.map((session) => {
            const style = session.context_type && CONTEXT_STYLES[session.context_type as ContextType];
            const folder = session.folder_id ? folderById.get(session.folder_id) : null;
            const tags = parseTags(session.tags);
            return (
              <button
                key={session.id}
                onClick={() => router.push(`/meeting-details?id=${session.id}`)}
                className="w-full text-left px-4 py-3 hover:bg-gray-50 flex items-center gap-3"
              >
                {style && (
                  <span className={`shrink-0 inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border text-xs font-medium ${style.chip}`}>
                    <span className={`w-1.5 h-1.5 rounded-full ${style.dot}`} />
                    {style.label}
                  </span>
                )}
                <span className="flex-1 truncate font-medium text-gray-800">{session.title}</span>
                {folder && (
                  <span className="shrink-0 flex items-center gap-1 text-xs text-gray-400">
                    <FolderIcon className="w-3 h-3" />
                    {folder.name}
                  </span>
                )}
                {tags.length > 0 && (
                  <span className="shrink-0 text-xs text-gray-400 hidden sm:inline">
                    {tags.map((t) => `#${t}`).join(' ')}
                  </span>
                )}
                {session.created_at && (
                  <span className="shrink-0 text-xs text-gray-400 w-20 text-right">
                    {formatDistanceToNow(new Date(session.created_at), { addSuffix: true })}
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
