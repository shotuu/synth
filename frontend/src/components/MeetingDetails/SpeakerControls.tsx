"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Loader2, Users } from 'lucide-react';
import { speakerColor } from '@/lib/speaker-colors';

interface SpeakerInfo {
  label: string;
  segment_count: number;
}

interface DiarizationProgress {
  meeting_id: string;
  stage: string;
  progress: number;
}

const STAGE_LABELS: Record<string, string> = {
  'downloading-models': 'Downloading speaker models…',
  'identifying-speakers': 'Identifying speakers…',
  'labeling-transcript': 'Labeling transcript…',
};

/**
 * "Identify speakers" action + per-speaker rename chips (Phase 4).
 * Diarization runs in the Rust core; progress arrives as Tauri events.
 */
export function SpeakerControls({
  meetingId,
  onTranscriptChanged,
}: {
  meetingId: string;
  onTranscriptChanged?: () => Promise<void> | void;
}) {
  const [speakers, setSpeakers] = useState<SpeakerInfo[]>([]);
  const [progress, setProgress] = useState<DiarizationProgress | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');

  const loadSpeakers = async (id: string) => {
    try {
      setSpeakers(await invoke<SpeakerInfo[]>('api_get_speakers', { meetingId: id }));
    } catch (error) {
      console.error('Failed to load speakers:', error);
    }
  };

  useEffect(() => {
    setSpeakers([]);
    setProgress(null);
    setRenaming(null);
    loadSpeakers(meetingId);

    const unlisteners = [
      listen<DiarizationProgress>('diarization-progress', (event) => {
        if (event.payload.meeting_id === meetingId) setProgress(event.payload);
      }),
      listen<{ meeting_id: string; speakers: number }>('diarization-complete', async (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        setProgress(null);
        toast.success(`Speakers identified: found ${event.payload.speakers}`);
        await loadSpeakers(meetingId);
        await onTranscriptChanged?.();
      }),
      listen<{ meeting_id: string; message: string }>('diarization-error', (event) => {
        if (event.payload.meeting_id !== meetingId) return;
        setProgress(null);
        toast.error(`Speaker identification failed: ${event.payload.message}`);
      }),
    ];

    return () => {
      unlisteners.forEach((p) => p.then((unlisten) => unlisten()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meetingId]);

  const identify = async () => {
    try {
      setProgress({ meeting_id: meetingId, stage: 'identifying-speakers', progress: 0 });
      await invoke('api_identify_speakers', { meetingId });
    } catch (error) {
      setProgress(null);
      toast.error(String(error));
    }
  };

  const commitRename = async (fromLabel: string) => {
    const to = renameValue.trim();
    setRenaming(null);
    if (!to || to === fromLabel) return;
    try {
      await invoke<number>('api_rename_speaker', { meetingId, fromLabel, toLabel: to });
      toast.success(`Renamed ${fromLabel} to ${to}`);
      await loadSpeakers(meetingId);
      await onTranscriptChanged?.();
    } catch (error) {
      toast.error(String(error));
    }
  };

  if (progress) {
    return (
      <div className="flex items-center gap-2 text-xs text-gray-500">
        <Loader2 className="w-3.5 h-3.5 animate-spin" />
        {STAGE_LABELS[progress.stage] ?? 'Working…'} {progress.progress > 0 && `${progress.progress}%`}
      </div>
    );
  }

  if (speakers.length === 0) {
    return (
      <button
        onClick={identify}
        className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border border-gray-200 text-xs font-medium text-gray-600 hover:bg-gray-50"
        title="Detect who spoke when, and label the transcript"
      >
        <Users className="w-3.5 h-3.5" />
        Identify speakers
      </button>
    );
  }

  return (
    <div className="flex items-center gap-1.5 flex-wrap min-w-0">
      {speakers.map((s) => {
        const color = speakerColor(s.label);
        return renaming === s.label ? (
          <input
            key={s.label}
            autoFocus
            defaultValue={s.label}
            onChange={(e) => setRenameValue(e.target.value)}
            onBlur={() => commitRename(s.label)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename(s.label);
              if (e.key === 'Escape') setRenaming(null);
            }}
            className="px-1.5 py-0.5 text-[11px] border border-gray-300 rounded w-24 focus:outline-none focus:ring-1 focus:ring-blue-400"
          />
        ) : (
          <button
            key={s.label}
            onClick={() => {
              setRenameValue(s.label);
              setRenaming(s.label);
            }}
            className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] font-medium ${color.chip} hover:opacity-80`}
            title={`${s.segment_count} segments — click to rename`}
          >
            <span className={`w-1.5 h-1.5 rounded-full ${color.dot}`} />
            {s.label}
          </button>
        );
      })}
      <button
        onClick={identify}
        className="text-[11px] text-gray-400 hover:text-gray-600 px-1"
        title="Run speaker identification again"
      >
        re-run
      </button>
    </div>
  );
}
