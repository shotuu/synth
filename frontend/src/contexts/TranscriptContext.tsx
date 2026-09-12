'use client';

import React, { createContext, useContext, useState, useEffect, useRef, useCallback, ReactNode, MutableRefObject } from 'react';
import { Transcript, TranscriptUpdate } from '@/types';
import { toast } from 'sonner';
import { useRecordingState } from './RecordingStateContext';
import { transcriptService } from '@/services/transcriptService';
import { recordingService } from '@/services/recordingService';
import { indexedDBService } from '@/services/indexedDBService';

// Only the noisy, per-chunk debug logs go through this - errors/warnings and
// one-time setup/cleanup logs always print regardless of build mode.
const debugLog = process.env.NODE_ENV !== 'production' ? console.log : () => {};

// Single canonical ordering for transcripts: chunk_start_time first, then
// sequence_id as a tiebreaker. Every place in this file that sorts
// transcripts (the buffered listener path and addTranscript) must use this
// same comparator, or the two paths can silently disagree on ordering.
function compareTranscripts(a: Transcript, b: Transcript): number {
  const chunkTimeDiff = (a.chunk_start_time || 0) - (b.chunk_start_time || 0);
  if (chunkTimeDiff !== 0) return chunkTimeDiff;
  return (a.sequence_id || 0) - (b.sequence_id || 0);
}

// prev is always already sorted (it's only ever produced by this function or
// by a full backend re-sync), so merging in a freshly-sorted batch is O(n)
// instead of re-sorting the entire combined history on every flush.
function mergeSortedTranscripts(prev: Transcript[], newBatch: Transcript[]): Transcript[] {
  if (newBatch.length === 0) return prev;
  const sortedBatch = [...newBatch].sort(compareTranscripts);
  const merged: Transcript[] = [];
  let i = 0;
  let j = 0;
  while (i < prev.length && j < sortedBatch.length) {
    if (compareTranscripts(prev[i], sortedBatch[j]) <= 0) {
      merged.push(prev[i++]);
    } else {
      merged.push(sortedBatch[j++]);
    }
  }
  while (i < prev.length) merged.push(prev[i++]);
  while (j < sortedBatch.length) merged.push(sortedBatch[j++]);
  return merged;
}

interface TranscriptContextType {
  transcripts: Transcript[];
  transcriptsRef: MutableRefObject<Transcript[]>
  addTranscript: (update: TranscriptUpdate) => void;
  copyTranscript: () => void;
  flushBuffer: () => void;
  transcriptContainerRef: React.RefObject<HTMLDivElement>;
  meetingTitle: string;
  setMeetingTitle: (title: string) => void;
  clearTranscripts: () => void;
  currentMeetingId: string | null;
  markMeetingAsSaved: () => Promise<void>;
}

const TranscriptContext = createContext<TranscriptContextType | undefined>(undefined);

export function TranscriptProvider({ children }: { children: ReactNode }) {
  const [transcripts, setTranscripts] = useState<Transcript[]>([]);
  const [meetingTitle, setMeetingTitle] = useState('+ New Call');
  const [currentMeetingId, setCurrentMeetingId] = useState<string | null>(null);

  // Recording state context - provides backend-synced state
  const recordingState = useRecordingState();

  // Refs for transcript management
  const transcriptsRef = useRef<Transcript[]>(transcripts);
  const isUserAtBottomRef = useRef<boolean>(true);
  const transcriptContainerRef = useRef<HTMLDivElement>(null);
  const finalFlushRef = useRef<(() => void) | null>(null);

  // currentMeetingId mirrored into a ref so the mount-once transcript listener effect
  // below can read the *current* meeting id without taking a dependency on it (see that
  // effect for why re-running on every meeting id change is unsafe).
  const currentMeetingIdRef = useRef<string | null>(currentMeetingId);
  useEffect(() => {
    currentMeetingIdRef.current = currentMeetingId;
  }, [currentMeetingId]);

  // Sequence-buffering state for the transcript listener, held in refs (not effect-local
  // `let`s) so it can be shared between the mount-once listener effect and the
  // per-meeting reset effect below.
  const transcriptBufferRef = useRef(new Map<number, Transcript>());
  const transcriptCounterRef = useRef(0);
  const lastProcessedSequenceRef = useRef(0);
  const processingTimerRef = useRef<NodeJS.Timeout | undefined>(undefined);

  // Tracks whether we've already shown the user a one-time warning this
  // recording session about IndexedDB crash-recovery writes failing.
  const indexedDbWriteWarnedRef = useRef(false);

  // Reset per-meeting buffering state whenever the active meeting changes, so stale
  // sequence numbers / dedup state from a previous meeting can't leak into a new one.
  useEffect(() => {
    transcriptBufferRef.current.clear();
    transcriptCounterRef.current = 0;
    lastProcessedSequenceRef.current = 0;
    indexedDbWriteWarnedRef.current = false;
    if (processingTimerRef.current) {
      clearTimeout(processingTimerRef.current);
      processingTimerRef.current = undefined;
    }
  }, [currentMeetingId]);

  // Keep ref updated with current transcripts
  useEffect(() => {
    transcriptsRef.current = transcripts;
  }, [transcripts]);

  // Smart auto-scroll: Track user scroll position
  useEffect(() => {
    const handleScroll = () => {
      const container = transcriptContainerRef.current;
      if (!container) return;

      const { scrollTop, scrollHeight, clientHeight } = container;
      const isAtBottom = scrollTop + clientHeight >= scrollHeight - 10; // 10px tolerance
      isUserAtBottomRef.current = isAtBottom;
    };

    const container = transcriptContainerRef.current;
    if (container) {
      container.addEventListener('scroll', handleScroll);
      return () => container.removeEventListener('scroll', handleScroll);
    }
  }, []);

  // Auto-scroll when transcripts change (only if user is at bottom)
  useEffect(() => {
    // Only auto-scroll if user was at the bottom before new content
    if (isUserAtBottomRef.current && transcriptContainerRef.current) {
      // Wait for Framer Motion animation to complete (150ms) before scrolling
      // This ensures scrollHeight includes the full rendered height of the new transcript
      const scrollTimeout = setTimeout(() => {
        const container = transcriptContainerRef.current;
        if (container) {
          container.scrollTo({
            top: container.scrollHeight,
            behavior: 'smooth'
          });
        }
      }, 150); // Match Framer Motion transition duration

      return () => clearTimeout(scrollTimeout);
    }
  }, [transcripts]);

  // Initialize IndexedDB and listen for recording-started/stopped events.
  // Mount-once (deps: []), same reasoning as the transcript listener effect
  // below: this used to depend on [currentMeetingId], but its own
  // recording-started handler calls setCurrentMeetingId, so every recording
  // start tore down and re-subscribed both listeners. A 'recording-stopped'
  // event that arrived during that async re-subscription window (e.g. a very
  // short recording) could be missed entirely, leaving folder_path metadata
  // never persisted. The recording-stopped handler below reads the current
  // meeting id via currentMeetingIdRef instead of closing over the state
  // variable, since this effect no longer re-runs per meeting.
  useEffect(() => {
    let unlistenRecordingStarted: (() => void) | undefined;
    let unlistenRecordingStopped: (() => void) | undefined;
    let cleanedUp = false;

    const setupRecordingListeners = async () => {
      try {
        // Initialize IndexedDB
        await indexedDBService.init();

        // Listen for recording-started event
        const startedUnlisten = await recordingService.onRecordingStarted(async () => {
          try {
            // Generate unique meeting ID
            const meetingId = `meeting-${Date.now()}`;
            setCurrentMeetingId(meetingId);
            // Also update the ref synchronously (not via the state-sync effect,
            // which only runs on the next render) so the recording-stopped
            // handler below sees the new id immediately, even if stop fires
            // before React commits this state update.
            currentMeetingIdRef.current = meetingId;

            // Store in sessionStorage as fallback for markMeetingAsSaved
            sessionStorage.setItem('indexeddb_current_meeting_id', meetingId);
            console.log('[Recording Started] 💾 IndexedDB meeting ID stored:', meetingId);

            // Get meeting name
            const meetingName = await recordingService.getRecordingMeetingName();

            // Use a better fallback that matches the backend's naming pattern
            const effectiveTitle = meetingName || `Meeting ${new Date().toISOString().slice(0, 19).replace('T', '_').replace(/:/g, '-')}`;

            // Initialize meeting metadata in IndexedDB
            await indexedDBService.saveMeetingMetadata({
              meetingId,
              title: effectiveTitle,
              startTime: Date.now(),
              lastUpdated: Date.now(),
              transcriptCount: 0,
              savedToSQLite: false,
              folderPath: undefined // Will update shortly
            });

            // Synchronize meeting title to state (fixes tray stop title issue)
            setMeetingTitle(effectiveTitle);

            // Fetch folder path from backend and update metadata
            // This ensures folder path is persisted even if app crashes
            try {
              const { invoke } = await import('@tauri-apps/api/core');
              const folderPath = await invoke<string>('get_meeting_folder_path');
              if (folderPath) {
                const metadata = await indexedDBService.getMeetingMetadata(meetingId);
                if (metadata) {
                  metadata.folderPath = folderPath;
                  await indexedDBService.saveMeetingMetadata(metadata);
                }
              }
            } catch (error) {
              // Non-fatal - will be set on stop if recording completes normally
            }
          } catch (error) {
            console.error('Failed to initialize meeting in IndexedDB:', error);
          }
        });
        if (cleanedUp) {
          startedUnlisten();
          return;
        }
        unlistenRecordingStarted = startedUnlisten;

        // Listen for recording-stopped event
        const stoppedUnlisten = await recordingService.onRecordingStopped(async (payload) => {
          try {
            const meetingId = currentMeetingIdRef.current;
            if (meetingId) {
              // Update folder path in IndexedDB
              const metadata = await indexedDBService.getMeetingMetadata(meetingId);

              if (metadata && payload.folder_path) {
                metadata.folderPath = payload.folder_path;
                await indexedDBService.saveMeetingMetadata(metadata);
              }
            }
          } catch (error) {
            console.error('Failed to update meeting metadata on stop:', error);
          }
        });
        if (cleanedUp) {
          stoppedUnlisten();
          return;
        }
        unlistenRecordingStopped = stoppedUnlisten;
      } catch (error) {
        console.error('Failed to setup recording listeners:', error);
      }
    };

    setupRecordingListeners();

    return () => {
      cleanedUp = true;
      if (unlistenRecordingStarted) {
        unlistenRecordingStarted();
        console.log('🧹 Recording started listener cleaned up');
      }
      if (unlistenRecordingStopped) {
        unlistenRecordingStopped();
        console.log('🧹 Recording stopped listener cleaned up');
      }
    };
  }, []); // Mount-once: see comment above the effect for why this must not depend on currentMeetingId

  // Main transcript buffering logic with sequence_id ordering. Registered once on
  // mount (deps: []) rather than re-subscribing per meeting — re-subscribing on every
  // currentMeetingId change used to unsubscribe synchronously but re-subscribe
  // asynchronously (transcriptService.onTranscriptUpdate returns a Promise), leaving a
  // real window with no listener registered right as a new recording starts and the
  // backend begins emitting events, silently dropping the first transcript chunks.
  // Per-meeting state (buffer/counters) lives in refs reset by the effect above instead
  // of effect-local `let`s, since this effect itself no longer re-runs per meeting.
  useEffect(() => {
    let unlistenFn: (() => void) | undefined;
    let cleanedUp = false;
    const transcriptBuffer = transcriptBufferRef.current;

    const processBufferedTranscripts = (forceFlush = false) => {
      const sortedTranscripts: Transcript[] = [];

      // Process all available sequential transcripts
      let nextSequence = lastProcessedSequenceRef.current + 1;
      while (transcriptBuffer.has(nextSequence)) {
        const bufferedTranscript = transcriptBuffer.get(nextSequence)!;
        sortedTranscripts.push(bufferedTranscript);
        transcriptBuffer.delete(nextSequence);
        lastProcessedSequenceRef.current = nextSequence;
        nextSequence++;
      }

      // Add any buffered transcripts that might be out of order
      const now = Date.now();
      const staleThreshold = 100;  // 100ms safety net only (serial workers = sequential order)
      const recentThreshold = 0;    // Show immediately - no delay needed with serial processing
      const staleTranscripts: Transcript[] = [];
      const recentTranscripts: Transcript[] = [];
      const forceFlushTranscripts: Transcript[] = [];

      for (const [sequenceId, transcript] of transcriptBuffer.entries()) {
        if (forceFlush) {
          // Force flush mode: process ALL remaining transcripts regardless of timing
          forceFlushTranscripts.push(transcript);
          transcriptBuffer.delete(sequenceId);
          debugLog(`Force flush: processing transcript with sequence_id ${sequenceId}`);
        } else {
          const transcriptAge = now - parseInt(transcript.id.split('-')[0]);
          if (transcriptAge > staleThreshold) {
            // Process stale transcripts (>100ms old - safety net)
            staleTranscripts.push(transcript);
            transcriptBuffer.delete(sequenceId);
          } else if (transcriptAge >= recentThreshold) {
            // Process immediately (0ms threshold with serial workers)
            recentTranscripts.push(transcript);
            transcriptBuffer.delete(sequenceId);
            debugLog(`Processing transcript with sequence_id ${sequenceId}, age: ${transcriptAge}ms`);
          }
        }
      }

      // Sort both stale and recent transcripts by chunk_start_time, then by sequence_id
      const sortedStaleTranscripts = staleTranscripts.sort(compareTranscripts);
      const sortedRecentTranscripts = recentTranscripts.sort(compareTranscripts);
      const sortedForceFlushTranscripts = forceFlushTranscripts.sort(compareTranscripts);

      const allNewTranscripts = [...sortedTranscripts, ...sortedRecentTranscripts, ...sortedStaleTranscripts, ...sortedForceFlushTranscripts];

      if (allNewTranscripts.length > 0) {
        setTranscripts(prev => {
          // Create a set of existing sequence_ids for deduplication
          const existingSequenceIds = new Set(prev.map(t => t.sequence_id).filter(id => id !== undefined));

          // Filter out any new transcripts that already exist
          const uniqueNewTranscripts = allNewTranscripts.filter(transcript =>
            transcript.sequence_id !== undefined && !existingSequenceIds.has(transcript.sequence_id)
          );

          // Only combine if we have unique new transcripts
          if (uniqueNewTranscripts.length === 0) {
            debugLog('No unique transcripts to add - all were duplicates');
            return prev; // No new unique transcripts to add
          }

          debugLog(`Adding ${uniqueNewTranscripts.length} unique transcripts out of ${allNewTranscripts.length} received`);

          // prev is already sorted, so this is an O(n) merge rather than
          // re-sorting the entire transcript history on every flush (which,
          // for a long meeting with thousands of segments, made total sort
          // cost grow roughly O(n^2 log n) over the recording).
          return mergeSortedTranscripts(prev, uniqueNewTranscripts);
        });

        // Log the processing summary
        const logMessage = forceFlush
          ? `Force flush processed ${allNewTranscripts.length} transcripts (${sortedTranscripts.length} sequential, ${forceFlushTranscripts.length} forced)`
          : `Processed ${allNewTranscripts.length} transcripts (${sortedTranscripts.length} sequential, ${recentTranscripts.length} recent, ${staleTranscripts.length} stale)`;
        debugLog(logMessage);
      }
    };

    // Assign final flush function to ref for external access
    finalFlushRef.current = () => processBufferedTranscripts(true);

    const setupListener = async () => {
      try {
        console.log('🔥 Setting up MAIN transcript listener during component initialization...');
        const listenerUnlisten = await transcriptService.onTranscriptUpdate((update) => {
          const now = Date.now();
          debugLog('🎯 MAIN LISTENER: Received transcript update:', {
            sequence_id: update.sequence_id,
            text: update.text.substring(0, 50) + '...',
            timestamp: update.timestamp,
            is_partial: update.is_partial,
            received_at: new Date(now).toISOString(),
            buffer_size_before: transcriptBuffer.size
          });

          // Check for duplicate sequence_id before processing
          if (transcriptBuffer.has(update.sequence_id)) {
            debugLog('🚫 MAIN LISTENER: Duplicate sequence_id, skipping buffer:', update.sequence_id);
            return;
          }

          // Create transcript for buffer with NEW timestamp fields
          const newTranscript: Transcript = {
            id: `${Date.now()}-${transcriptCounterRef.current++}`,
            text: update.text,
            timestamp: update.timestamp,
            sequence_id: update.sequence_id,
            chunk_start_time: update.chunk_start_time,
            is_partial: update.is_partial,
            confidence: update.confidence,
            // NEW: Recording-relative timestamps for playback sync
            audio_start_time: update.audio_start_time,
            audio_end_time: update.audio_end_time,
            duration: update.duration,
          };

          // Add to buffer
          transcriptBuffer.set(update.sequence_id, newTranscript);
          debugLog(`✅ MAIN LISTENER: Buffered transcript with sequence_id ${update.sequence_id}. Buffer size: ${transcriptBuffer.size}, Last processed: ${lastProcessedSequenceRef.current}`);

          // Save to IndexedDB (non-blocking). Read the current meeting id from the ref
          // (not the closed-over state variable) since this effect only runs once.
          if (currentMeetingIdRef.current) {
            indexedDBService.saveTranscript(currentMeetingIdRef.current, update)
              .catch(err => {
                console.warn('IndexedDB save failed:', err);
                // Surface this to the user once per recording (reset on meeting
                // change above) rather than letting an entire session's worth of
                // crash-recovery writes fail silently with no visible signal.
                if (!indexedDbWriteWarnedRef.current) {
                  indexedDbWriteWarnedRef.current = true;
                  toast.error('Local backup of this recording is failing - your transcript is still live, but crash recovery may be incomplete.');
                }
              });
          }

          // Clear any existing timer and set a new one
          if (processingTimerRef.current) {
            clearTimeout(processingTimerRef.current);
          }

          // Process buffer with minimal delay for immediate UI updates (serial workers = sequential order)
          processingTimerRef.current = setTimeout(processBufferedTranscripts, 10);
        });
        if (cleanedUp) {
          listenerUnlisten();
          return;
        }
        unlistenFn = listenerUnlisten;
        console.log('✅ MAIN transcript listener setup complete');
      } catch (error) {
        console.error('❌ Failed to setup MAIN transcript listener:', error);
        alert('Failed to setup transcript listener. Check console for details.');
      }
    };

    setupListener();
    console.log('Started enhanced listener setup');

    return () => {
      cleanedUp = true;
      console.log('🧹 CLEANUP: Cleaning up MAIN transcript listener...');
      if (processingTimerRef.current) {
        clearTimeout(processingTimerRef.current);
        processingTimerRef.current = undefined;
        console.log('🧹 CLEANUP: Cleared processing timer');
      }
      if (unlistenFn) {
        unlistenFn();
        console.log('🧹 CLEANUP: MAIN transcript listener cleaned up');
      }
    };
  }, []); // Mount-once: see comment above the effect for why this must not depend on currentMeetingId

  // Sync transcript history and meeting name from backend on reload
  // This fixes the issue where reloading during active recording causes state desync
  useEffect(() => {
    const syncFromBackend = async () => {
      // If recording is active and we have no local transcripts, sync from backend.
      // Read transcriptsRef (always current) rather than the `transcripts` closure
      // captured when this effect fired - otherwise a live transcript arriving
      // while getTranscriptHistory() is in flight would be silently overwritten
      // below by backend history that no longer reflects "we have nothing yet".
      if (recordingState.isRecording && transcriptsRef.current.length === 0) {
        try {
          console.log('[Reload Sync] Recording active after reload, syncing transcript history...');

          // Fetch transcript history from backend
          const history = await transcriptService.getTranscriptHistory();
          console.log(`[Reload Sync] Retrieved ${history.length} transcript segments from backend`);

          // Convert backend format to frontend Transcript format
          const formattedTranscripts: Transcript[] = history.map((segment: any) => ({
            id: segment.id,
            text: segment.text,
            timestamp: segment.display_time, // Use display_time for UI
            sequence_id: segment.sequence_id,
            chunk_start_time: segment.audio_start_time,
            is_partial: false, // History segments are always final
            confidence: segment.confidence,
            audio_start_time: segment.audio_start_time,
            audio_end_time: segment.audio_end_time,
            duration: segment.duration,
          }));

          // Re-check right before writing: a live transcript may have arrived
          // via the main listener while the two awaits above were in flight.
          if (transcriptsRef.current.length === 0) {
            setTranscripts(formattedTranscripts);
            console.log('[Reload Sync] ✅ Transcript history synced successfully');
          } else {
            console.log('[Reload Sync] Skipping: live transcript(s) arrived during sync');
          }

          // Fetch meeting name from backend
          const meetingName = await recordingService.getRecordingMeetingName();
          if (meetingName) {
            console.log('[Reload Sync] Retrieved meeting name:', meetingName);
            setMeetingTitle(meetingName);
            console.log('[Reload Sync] ✅ Meeting title synced successfully');
          }
        } catch (error) {
          console.error('[Reload Sync] Failed to sync from backend:', error);
        }
      }
    };

    syncFromBackend();
  }, [recordingState.isRecording]); // Run when recording state changes

  // Manual transcript update handler (for RecordingControls component).
  // sequence_id is a required field on TranscriptUpdate, but the old id/sort
  // fallbacks here used `update.sequence_id ? ... : ...`, which treats a
  // valid sequence_id of 0 (the first chunk) as missing. Uses sequence_id
  // directly instead, and shares the same dedup key (sequence_id) and sort
  // order (compareTranscripts) as the buffered listener path above so the
  // two insertion paths can't silently disagree on ordering/dedup.
  const addTranscript = useCallback((update: TranscriptUpdate) => {
    debugLog('🎯 addTranscript called with:', {
      sequence_id: update.sequence_id,
      text: update.text.substring(0, 50) + '...',
      timestamp: update.timestamp,
      is_partial: update.is_partial
    });

    const newTranscript: Transcript = {
      id: update.sequence_id.toString(),
      text: update.text,
      timestamp: update.timestamp,
      sequence_id: update.sequence_id,
      chunk_start_time: update.chunk_start_time,
      is_partial: update.is_partial,
      confidence: update.confidence,
      audio_start_time: update.audio_start_time,
      audio_end_time: update.audio_end_time,
      duration: update.duration,
    };

    setTranscripts(prev => {
      // Check if this transcript already exists
      const exists = prev.some(t => t.sequence_id === update.sequence_id);
      if (exists) {
        debugLog('🚫 Duplicate transcript detected, skipping:', update.text.substring(0, 30) + '...');
        return prev;
      }

      const sorted = mergeSortedTranscripts(prev, [newTranscript]);
      debugLog('✅ Added new transcript. New count:', sorted.length);
      return sorted;
    });
  }, []);

  // Copy transcript to clipboard with recording-relative timestamps
  const copyTranscript = useCallback(() => {
    // Format timestamps as recording-relative [MM:SS] instead of wall-clock time
    const formatTime = (seconds: number | undefined): string => {
      if (seconds === undefined) return '[--:--]';
      const totalSecs = Math.floor(seconds);
      const mins = Math.floor(totalSecs / 60);
      const secs = totalSecs % 60;
      return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
    };

    const fullTranscript = transcripts
      .map(t => `${formatTime(t.audio_start_time)} ${t.text}`)
      .join('\n');
    navigator.clipboard.writeText(fullTranscript);

    toast.success("Transcript copied to clipboard");
  }, [transcripts]);

  // Force flush buffer (for final transcript processing)
  const flushBuffer = useCallback(() => {
    if (finalFlushRef.current) {
      console.log('🔄 Flushing transcript buffer...');
      finalFlushRef.current();
    }
  }, []);

  // Clear transcripts (used when starting new recording)
  const clearTranscripts = useCallback(() => {
    setTranscripts([]);
    // Don't clear currentMeetingId here - it will be set by recording-started event
  }, []);

  // Mark current meeting as saved in IndexedDB
  const markMeetingAsSaved = useCallback(async () => {
    // Try context state first, fallback to sessionStorage
    const meetingId = currentMeetingId || sessionStorage.getItem('indexeddb_current_meeting_id');

    if (!meetingId) {
      console.error('[IndexedDB] ❌ Cannot mark meeting as saved: No meeting ID available!');
      console.error('[IndexedDB] currentMeetingId:', currentMeetingId);
      console.error('[IndexedDB] sessionStorage:', sessionStorage.getItem('indexeddb_current_meeting_id'));
      return;
    }

    try {
      await indexedDBService.markMeetingSaved(meetingId);

      // Clear both sources
      setCurrentMeetingId(null);
      sessionStorage.removeItem('indexeddb_current_meeting_id');
    } catch (error) {
      console.error('[IndexedDB] ❌ Failed to mark meeting as saved:', error);
    }
  }, [currentMeetingId]);

  const value: TranscriptContextType = {
    transcripts,
    transcriptsRef,
    addTranscript,
    copyTranscript,
    flushBuffer,
    transcriptContainerRef,
    meetingTitle,
    setMeetingTitle,
    clearTranscripts,
    currentMeetingId,
    markMeetingAsSaved,
  };

  return (
    <TranscriptContext.Provider value={value}>
      {children}
    </TranscriptContext.Provider>
  );
}

export function useTranscripts() {
  const context = useContext(TranscriptContext);
  if (context === undefined) {
    throw new Error('useTranscripts must be used within a TranscriptProvider');
  }
  return context;
}
