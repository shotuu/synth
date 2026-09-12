"use client";

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { Home, Folder as FolderIcon, CheckSquare, Settings, FileText, HardDrive, Mic } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { CONTEXT_STYLES, ContextType } from '@/components/MeetingDetails/ContextTypeSelector';

interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
}

/**
 * Global Cmd/Ctrl+K palette (PROJECT_BRIEF.md §8): jump to a session or
 * folder, search transcript content, or trigger a quick action — without
 * leaving the keyboard.
 */
export function CommandPalette() {
  const router = useRouter();
  const { meetings, folders, handleRecordingToggle } = useSidebar();
  const { isRecording } = useRecordingState();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [transcriptResults, setTranscriptResults] = useState<TranscriptSearchResult[]>([]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setOpen((prev) => !prev);
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, []);

  // Debounced transcript search as the user types
  useEffect(() => {
    if (!open || query.trim().length < 2) {
      setTranscriptResults([]);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const results = await invoke<TranscriptSearchResult[]>('api_search_transcripts', { query });
        setTranscriptResults(results.slice(0, 8));
      } catch (error) {
        console.error('Command palette transcript search failed:', error);
        setTranscriptResults([]);
      }
    }, 250);
    return () => clearTimeout(timer);
  }, [query, open]);

  const go = (path: string) => {
    setOpen(false);
    setQuery('');
    router.push(path);
  };

  const filteredMeetings = query.trim()
    ? meetings.filter((m) => m.title.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 8)
    : meetings.slice(0, 6);

  const filteredFolders = query.trim()
    ? folders.filter((f) => f.name.toLowerCase().includes(query.trim().toLowerCase()))
    : folders;

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <CommandInput
        placeholder="Jump to a session, folder, or search transcripts..."
        value={query}
        onValueChange={setQuery}
      />
      <CommandList>
        <CommandEmpty>No results found.</CommandEmpty>

        {!query.trim() && (
          <CommandGroup heading="Go to">
            {!isRecording && (
              <CommandItem
                onSelect={() => {
                  setOpen(false);
                  setQuery('');
                  handleRecordingToggle();
                }}
              >
                <Mic className="w-4 h-4 mr-2" /> New session - start recording
              </CommandItem>
            )}
            <CommandItem onSelect={() => go('/')}>
              <Home className="w-4 h-4 mr-2" /> Home
            </CommandItem>
            <CommandItem onSelect={() => go('/sessions')}>
              <FolderIcon className="w-4 h-4 mr-2" /> All Sessions
            </CommandItem>
            <CommandItem onSelect={() => go('/action-items')}>
              <CheckSquare className="w-4 h-4 mr-2" /> Action Items
            </CommandItem>
            <CommandItem onSelect={() => go('/storage')}>
              <HardDrive className="w-4 h-4 mr-2" /> Storage Manager
            </CommandItem>
            <CommandItem onSelect={() => go('/settings')}>
              <Settings className="w-4 h-4 mr-2" /> Settings
            </CommandItem>
          </CommandGroup>
        )}

        {filteredFolders.length > 0 && (
          <CommandGroup heading="Folders">
            {filteredFolders.map((f) => (
              <CommandItem key={f.id} onSelect={() => go(`/sessions?folder=${f.id}`)}>
                <span className="mr-2">{f.icon ?? '📁'}</span>
                {f.name}
              </CommandItem>
            ))}
          </CommandGroup>
        )}

        {filteredMeetings.length > 0 && (
          <CommandGroup heading="Sessions">
            {filteredMeetings.map((m) => {
              const style = m.contextType && CONTEXT_STYLES[m.contextType as ContextType];
              return (
                <CommandItem key={m.id} onSelect={() => go(`/meeting-details?id=${m.id}`)}>
                  {style ? (
                    <span className={`w-2 h-2 rounded-full mr-2 shrink-0 ${style.dot}`} />
                  ) : (
                    <FileText className="w-4 h-4 mr-2 shrink-0" />
                  )}
                  {m.title}
                </CommandItem>
              );
            })}
          </CommandGroup>
        )}

        {transcriptResults.length > 0 && (
          <CommandGroup heading="Transcript matches">
            {transcriptResults.map((r) => (
              <CommandItem key={r.id} onSelect={() => go(`/meeting-details?id=${r.id}`)}>
                <FileText className="w-4 h-4 mr-2 shrink-0" />
                <div className="flex flex-col min-w-0">
                  <span className="truncate">{r.title}</span>
                  <span className="text-xs text-gray-400 truncate">{r.matchContext}</span>
                </div>
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}
