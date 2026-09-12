"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from '@/components/ui/dropdown-menu';

export type ContextType = 'meeting' | 'lecture' | 'discussion' | 'coffee_chat' | 'custom';

/** Context-type badge colors (PROJECT_BRIEF.md §8) */
export const CONTEXT_STYLES: Record<ContextType, { label: string; dot: string; chip: string }> = {
  meeting: { label: 'Meeting', dot: 'bg-blue-500', chip: 'bg-blue-50 text-blue-700 border-blue-200' },
  lecture: { label: 'Lecture', dot: 'bg-purple-500', chip: 'bg-purple-50 text-purple-700 border-purple-200' },
  discussion: { label: 'Discussion', dot: 'bg-emerald-500', chip: 'bg-emerald-50 text-emerald-700 border-emerald-200' },
  coffee_chat: { label: 'Coffee Chat', dot: 'bg-amber-500', chip: 'bg-amber-50 text-amber-700 border-amber-200' },
  custom: { label: 'Custom', dot: 'bg-gray-400', chip: 'bg-gray-50 text-gray-600 border-gray-200' },
};

export const CONTEXT_ORDER: ContextType[] = ['meeting', 'lecture', 'discussion', 'coffee_chat', 'custom'];

/**
 * Session-type badge + picker. The type is auto-suggested after
 * transcription; this is where the user confirms or overrides it. Changing
 * it re-defaults the summary template via onContextChange.
 */
export function ContextTypeSelector({
  meetingId,
  onContextChange,
}: {
  meetingId: string;
  onContextChange?: (contextType: ContextType) => void;
}) {
  const [contextType, setContextType] = useState<ContextType | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<string>('api_get_context_type', { meetingId })
      .then((ct) => {
        if (cancelled) return;
        const valid = (CONTEXT_ORDER as string[]).includes(ct) ? (ct as ContextType) : 'meeting';
        setContextType(valid);
        onContextChange?.(valid);
      })
      .catch((error) => {
        console.error('Failed to load context type:', error);
        if (!cancelled) setContextType('meeting');
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meetingId]);

  const select = async (next: ContextType) => {
    if (next === contextType) return;
    const previous = contextType;
    setContextType(next);
    try {
      await invoke('api_set_context_type', { meetingId, contextType: next });
      onContextChange?.(next);
      toast.success(`Session type set to ${CONTEXT_STYLES[next].label}`);
    } catch (error) {
      console.error('Failed to set context type:', error);
      setContextType(previous);
      toast.error('Failed to change session type');
    }
  };

  if (!contextType) return null;

  const current = CONTEXT_STYLES[contextType];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border text-xs font-medium ${current.chip}`}
          title="Session type - drives the summary structure"
        >
          <span className={`w-1.5 h-1.5 rounded-full ${current.dot}`} />
          {current.label}
          <svg className="w-3 h-3 opacity-60" viewBox="0 0 12 12" fill="none">
            <path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[150px]">
        {CONTEXT_ORDER.map((ct) => (
          <DropdownMenuItem
            key={ct}
            onSelect={() => select(ct)}
            className={`gap-2 text-xs ${ct === contextType ? 'font-semibold' : ''}`}
          >
            <span className={`w-1.5 h-1.5 rounded-full ${CONTEXT_STYLES[ct].dot}`} />
            {CONTEXT_STYLES[ct].label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
