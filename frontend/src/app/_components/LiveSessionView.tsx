'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { Panel, PanelGroup, PanelResizeHandle, ImperativePanelHandle } from 'react-resizable-panels';
import type { Block, PartialBlock } from '@blocknote/core';
import { useCreateBlockNote } from '@blocknote/react';
import { BlockNoteView } from '@blocknote/shadcn';
import '@blocknote/shadcn/style.css';
import '@blocknote/core/fonts/inter.css';
import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { VirtualizedTranscriptView } from '@/components/VirtualizedTranscriptView';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { blocksToMarkdownSafely } from '@/lib/blocknote-markdown';
import { saveLiveNotesDraft, loadLiveNotesDraft, draftHasContent } from '@/lib/liveNotesStore';

function formatElapsed(totalSeconds: number): string {
  const mins = Math.floor(totalSeconds / 60);
  const secs = Math.floor(totalSeconds % 60);
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`;
}

function parseDraftBlocks(): PartialBlock[] | undefined {
  const draft = loadLiveNotesDraft();
  if (!draftHasContent(draft)) return undefined;
  try {
    const blocks = JSON.parse(draft.notesJson);
    return Array.isArray(blocks) && blocks.length > 0 ? blocks : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Notes editor for an in-progress recording. Every change is written
 * synchronously to the live-notes draft (localStorage) so a crash loses at
 * most the current keystroke; useRecordingStop flushes the draft into
 * meeting_notes once the save produces a meeting_id. If a previous session
 * crashed before its flush, that draft is the initial content here rather
 * than being silently discarded.
 */
function LiveNotesEditor() {
  const initialContent = useMemo(parseDraftBlocks, []);
  const editor = useCreateBlockNote({ initialContent });

  const handleChange = async () => {
    const blocks = editor.document as Block[];
    const result = await blocksToMarkdownSafely(editor, blocks, {
      source: 'live-session-notes',
    });
    saveLiveNotesDraft(JSON.stringify(blocks), result.markdown ?? '');
  };

  return <BlockNoteView editor={editor} onChange={handleChange} theme="dark" data-testid="live-notes-editor" />;
}

/**
 * The active-recording workspace (Phase 10): live transcript and your own
 * notes side by side, the way notes actually get taken in a lecture or
 * meeting — transcript as supplement, not sole source. The split is
 * draggable, persisted across sessions, and the transcript pane collapses
 * entirely for distraction-free writing.
 */
export function LiveSessionView({
  isProcessingStop,
  isStopping,
}: {
  isProcessingStop: boolean;
  isStopping: boolean;
}) {
  const { transcripts, meetingTitle } = useTranscripts();
  const { isRecording, isPaused, recordingDuration } = useRecordingState();
  const transcriptPanelRef = useRef<ImperativePanelHandle>(null);
  const [transcriptCollapsed, setTranscriptCollapsed] = useState(false);

  // Elapsed ticker: recordingDuration from the context is authoritative but
  // updates coarsely; tick locally each second while recording for a live feel.
  const [elapsed, setElapsed] = useState(recordingDuration ?? 0);
  useEffect(() => {
    setElapsed(recordingDuration ?? 0);
  }, [recordingDuration]);
  useEffect(() => {
    if (!isRecording || isPaused) return;
    const t = setInterval(() => setElapsed((s) => s + 1), 1000);
    return () => clearInterval(t);
  }, [isRecording, isPaused]);

  const segments = useMemo(
    () =>
      transcripts.map((t) => ({
        id: t.id,
        timestamp: t.audio_start_time ?? 0,
        endTime: t.audio_end_time,
        text: t.text,
        confidence: t.confidence,
      })),
    [transcripts]
  );

  const toggleTranscript = () => {
    const panel = transcriptPanelRef.current;
    if (!panel) return;
    if (panel.isCollapsed()) {
      panel.expand();
    } else {
      panel.collapse();
    }
  };

  return (
    <div className="flex flex-col flex-1 min-w-0 h-screen bg-gray-50">
      {/* Session header: what's being recorded, for how long. Left-padded
          (pl-12) to clear the floating sidebar-reopen button when the
          sidebar is hidden — same fix as meeting-details/page-content.tsx.
          The transcript-panel toggle lives on the right, grouped with the
          other session controls, so it never sits next to the sidebar
          button (both are near-identical panel-chevron icons — adjacent,
          they read as a confusing double control rather than two distinct
          things). */}
      <div className="flex items-center gap-3 pl-12 pr-5 py-3 border-b border-gray-200 shrink-0">
        <h1 className="text-sm font-semibold text-gray-900 truncate flex-1">{meetingTitle}</h1>
        <div className="flex items-center gap-2 shrink-0">
          <span
            className={`w-2 h-2 rounded-full ${isPaused ? 'bg-orange-500' : 'bg-red-500 animate-pulse'}`}
          />
          <span className="font-mono text-xs text-gray-600">
            {isPaused ? 'Paused' : 'Recording'} · {formatElapsed(elapsed)}
          </span>
        </div>
        <div className="w-px h-4 bg-gray-200 shrink-0" />
        <button
          onClick={toggleTranscript}
          className="text-gray-400 hover:text-gray-700 transition-colors shrink-0"
          title={transcriptCollapsed ? 'Show live transcript' : 'Hide transcript - write distraction-free'}
        >
          {transcriptCollapsed ? <PanelLeftOpen className="w-4 h-4" /> : <PanelLeftClose className="w-4 h-4" />}
        </button>
      </div>

      <PanelGroup direction="horizontal" autoSaveId="synth-live-session" className="flex-1 min-h-0">
        <Panel
          ref={transcriptPanelRef}
          id="live-transcript"
          order={1}
          collapsible
          collapsedSize={0}
          defaultSize={42}
          minSize={22}
          onCollapse={() => setTranscriptCollapsed(true)}
          onExpand={() => setTranscriptCollapsed(false)}
          className="flex flex-col min-w-0"
        >
          {/* flex-col + min-h-0 gives VirtualizedTranscriptView's own
              internal `h-full overflow-y-auto` div a real bounded height to
              scroll within. It used to be nested inside a second
              overflow-y-auto div here — that outer div silently became the
              one actually scrolling (h-full on the inner one had nothing to
              resolve against), so useAutoScroll's scrollRef pointed at an
              element with no real overflow and new transcript text never
              auto-scrolled into view. */}
          <div className="flex flex-col h-full bg-white/50">
            <div className="text-[11px] uppercase tracking-wider text-gray-400 px-5 pt-4 pb-2 select-none shrink-0">
              Live transcript
            </div>
            <div className="flex-1 min-h-0">
              <VirtualizedTranscriptView
                segments={segments}
                isRecording={isRecording}
                isPaused={isPaused}
                isProcessing={isProcessingStop}
                isStopping={isStopping}
                enableStreaming={isRecording}
                showConfidence={false}
                hideStatusBar
              />
            </div>
          </div>
        </Panel>

        <PanelResizeHandle className="w-px bg-gray-200 hover:bg-blue-500 data-[resize-handle-state=drag]:bg-blue-500 transition-colors" />

        <Panel id="live-notes" order={2} minSize={30} className="flex flex-col min-w-0">
          <div className="flex-1 overflow-y-auto custom-scrollbar">
            <div className="px-5 pt-4 pb-24 max-w-[760px]">
              <div className="text-[11px] uppercase tracking-wider text-gray-400 mb-2 select-none">
                Your notes <span className="normal-case tracking-normal">- saved with this session when you stop</span>
              </div>
              <LiveNotesEditor />
            </div>
          </div>
        </Panel>
      </PanelGroup>
    </div>
  );
}
