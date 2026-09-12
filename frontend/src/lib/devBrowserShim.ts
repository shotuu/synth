/**
 * Dev-only Tauri bridge shim for plain-browser preview (pnpm run dev opened
 * in a browser instead of the Tauri webview). Outside the native shell,
 * `window.__TAURI_INTERNALS__` doesn't exist and every invoke() throws,
 * which forces the app into onboarding and blanks every page — making UI
 * work unverifiable in a browser. This installs a mock bridge with fixture
 * data so screens render with realistic content.
 *
 * Never active in the real app: Tauri always injects __TAURI_INTERNALS__
 * before any page script runs, and the whole module is a no-op outside
 * development builds.
 *
 * Import for side effects at the top of app/layout.tsx.
 */

type InvokeArgs = Record<string, unknown> | undefined;

const NOW = new Date();
const daysAgo = (n: number) => new Date(NOW.getTime() - n * 86400000).toISOString();

interface FixtureFolder {
  id: string;
  parent_folder_id: string | null;
  name: string;
  icon: string | null;
  sort_order: number;
}

const FOLDERS: FixtureFolder[] = [
  { id: 'folder-classes', parent_folder_id: null, name: 'Classes', icon: null, sort_order: 0 },
  { id: 'folder-work', parent_folder_id: null, name: 'Work', icon: null, sort_order: 1 },
  { id: 'folder-cs229', parent_folder_id: 'folder-classes', name: 'CS 229', icon: null, sort_order: 0 },
];

interface FixtureMeeting {
  id: string;
  title: string;
  context_type: string;
  folder_id: string | null;
  tags: string | null;
  created_at: string;
}

const MEETINGS: FixtureMeeting[] = [
  { id: 'demo-1', title: 'CS 229 Lecture 12 - Kernel Methods', context_type: 'lecture', folder_id: 'folder-cs229', tags: '["ml","kernels"]', created_at: daysAgo(0.2) },
  { id: 'demo-2', title: 'Sprint Planning - Q3 Roadmap', context_type: 'meeting', folder_id: 'folder-work', tags: '["planning"]', created_at: daysAgo(1) },
  { id: 'demo-3', title: 'Design Review: Export Pipeline', context_type: 'meeting', folder_id: 'folder-work', tags: '["design","exports"]', created_at: daysAgo(2) },
  { id: 'demo-4', title: 'Office Hours - Problem Set 6', context_type: 'lecture', folder_id: 'folder-cs229', tags: null, created_at: daysAgo(4) },
  { id: 'demo-5', title: 'Untitled Session', context_type: 'other', folder_id: null, tags: null, created_at: daysAgo(7) },
];

const ACTION_ITEMS = [
  { id: 'ai-1', meeting_id: 'demo-2', description: 'Draft the Q3 OKR doc and circulate before Friday', owner: 'Dan', due_date: '2026-07-11', done: false, created_at: daysAgo(1), meeting_title: 'Sprint Planning - Q3 Roadmap', folder_id: 'folder-work', context_type: 'meeting' },
  { id: 'ai-2', meeting_id: 'demo-2', description: 'File the infra ticket for the staging cluster', owner: 'Priya', due_date: null, done: false, created_at: daysAgo(1), meeting_title: 'Sprint Planning - Q3 Roadmap', folder_id: 'folder-work', context_type: 'meeting' },
  { id: 'ai-3', meeting_id: 'demo-4', description: 'Finish problem set 6, question 3 (SVM duality)', owner: null, due_date: '2026-07-08', done: true, created_at: daysAgo(4), meeting_title: 'Office Hours - Problem Set 6', folder_id: 'folder-cs229', context_type: 'lecture' },
];

const TRANSCRIPT_SEGMENTS = [
  { text: "Okay, let's pick up where we left off with kernel methods.", speaker: 'Speaker 1', start: 0.0 },
  { text: 'The key idea is that we never need the feature map explicitly.', speaker: 'Speaker 1', start: 6.5 },
  { text: 'So the kernel trick lets us work in infinite dimensions?', speaker: 'Speaker 2', start: 14.2 },
  { text: 'Exactly - as long as the kernel satisfies Mercer’s condition.', speaker: 'Speaker 1', start: 18.9 },
];

const SUMMARY_MARKDOWN = [
  '**Key Concepts**',
  '',
  '- The kernel trick replaces explicit feature maps with inner products',
  '- Mercer’s condition guarantees a valid reproducing kernel Hilbert space',
  '',
  '**Action Items**',
  '',
  '| Owner | Task | Due |',
  '| --- | --- | --- |',
  '| Dan | Review SVM duality notes | Friday |',
].join('\n');

function fixture(cmd: string, args: InvokeArgs): unknown {
  switch (cmd) {
    case 'get_onboarding_status':
      return {
        completed: true,
        current_step: 4,
        model_status: { parakeet: 'downloaded', summary: 'downloaded', selected_summary_model: 'llama3.1:latest' },
      };
    case 'check_first_launch':
      return false;
    // Set localStorage.synthShimRecording = '1' in the browser console to
    // preview the live-recording workspace without a native audio pipeline.
    case 'is_recording':
      return localStorage.getItem('synthShimRecording') === '1';
    case 'get_recording_state': {
      const rec = localStorage.getItem('synthShimRecording') === '1';
      return { is_recording: rec, is_paused: false, is_active: rec, recording_duration: rec ? 754 : 0, active_duration: rec ? 754 : 0 };
    }
    case 'get_audio_devices':
      return [
        { name: 'MacBook Pro Microphone', device_type: 'Input' },
        { name: 'BlackHole 2ch', device_type: 'Output' },
      ];
    case 'get_ollama_models':
      return [{ name: 'llama3.1:latest', id: 'llama3.1', size: '4.9 GB', modified: '2 weeks ago' }];
    case 'get_database_directory':
      return '/demo/Application Support/com.synth.app';
    case 'whisper_get_models_directory':
      return '/demo/models';
    case 'get_default_recordings_folder_path':
      return '/demo/Movies/synth-recordings';
    case 'get_recording_preferences':
      return {
        auto_save: true,
        file_format: 'mp4',
        save_folder: '/demo/Movies/synth-recordings',
        preferred_mic_device: 'MacBook Pro Microphone',
        preferred_system_device: 'BlackHole 2ch',
      };
    // Fresh shallow copies each call, matching real Tauri IPC (every invoke
    // deserializes new JSON into new objects) — returning the live array
    // reference made React's setState(sameRef) bail out as a no-op after
    // in-place mutations below, masking real updates in browser testing.
    case 'api_get_meetings':
      return MEETINGS.map((m) => ({ ...m }));
    case 'api_list_folders':
      return FOLDERS.map((f) => ({ ...f }));
    // Folder tree mutations: mutate the fixture arrays in place (mirroring
    // real persistence) so refetch-after-drop actually shows the move —
    // otherwise every drag-and-drop looks like a no-op against static fixtures.
    case 'api_set_meeting_folder': {
      const m = MEETINGS.find((mm) => mm.id === (args?.meetingId as string));
      if (m) m.folder_id = (args?.folderId as string | null) ?? null;
      return null;
    }
    case 'api_move_folder': {
      const f = FOLDERS.find((ff) => ff.id === (args?.folderId as string));
      if (f) f.parent_folder_id = (args?.newParentId as string | null) ?? null;
      return null;
    }
    case 'api_create_folder': {
      const id = `folder-${Math.random().toString(36).slice(2, 8)}`;
      const created: FixtureFolder = {
        id,
        parent_folder_id: (args?.parentFolderId as string | null) ?? null,
        name: (args?.name as string) ?? 'New folder',
        icon: (args?.icon as string | null) ?? null,
        sort_order: FOLDERS.length,
      };
      FOLDERS.push(created);
      return created;
    }
    case 'api_rename_folder': {
      const f = FOLDERS.find((ff) => ff.id === (args?.folderId as string));
      if (f) f.name = (args?.name as string) ?? f.name;
      return true;
    }
    case 'api_delete_folder': {
      const idx = FOLDERS.findIndex((ff) => ff.id === (args?.folderId as string));
      if (idx >= 0) FOLDERS.splice(idx, 1);
      MEETINGS.forEach((m) => {
        if (m.folder_id === (args?.folderId as string)) {
          m.folder_id = null;
        }
      });
      return null;
    }
    case 'api_list_action_items':
      return ACTION_ITEMS;
    case 'api_get_storage_stats':
      return { audio_bytes: 1.8e9, attachments_bytes: 2.4e8, database_bytes: 5.2e7, session_count: MEETINGS.length, sessions_with_retained_audio: 3 };
    case 'api_list_session_audio':
    case 'api_suggested_cleanup':
      return MEETINGS.slice(0, 3).map((m, i) => ({
        meeting_id: m.id,
        title: m.title,
        context_type: m.context_type,
        folder_id: m.folder_id,
        created_at: m.created_at,
        updated_at: m.created_at,
        current_size_bytes: [6.4e8, 7.1e8, 4.5e8][i],
        bitrate_kbps: 128,
        retained: true,
        last_compressed_at: null,
        storage_path: `/demo/recordings/${m.id}.mp4`,
      }));
    case 'api_get_meeting_metadata': {
      const id = (args?.meetingId as string) ?? 'demo-1';
      const m = MEETINGS.find(mm => mm.id === id) ?? MEETINGS[0];
      return { id: m.id, title: m.title, created_at: m.created_at, updated_at: m.created_at, context_type: m.context_type, folder_id: m.folder_id, tags: m.tags };
    }
    case 'api_get_meeting_transcripts':
      return {
        transcripts: TRANSCRIPT_SEGMENTS.map((s, i) => ({
          id: `t-${i}`,
          text: s.text,
          timestamp: '14:30:05',
          sequence_id: i,
          audio_start_time: s.start,
          audio_end_time: s.start + 4,
          duration: 4,
          speaker: s.speaker,
        })),
        total_count: TRANSCRIPT_SEGMENTS.length,
        has_more: false,
      };
    case 'api_get_summary':
      return { status: 'completed', data: { markdown: SUMMARY_MARKDOWN }, error: null };
    case 'api_get_context_type':
      return 'lecture';
    case 'api_get_speakers':
      return [
        { id: 'sp-1', label: 'Speaker 1', display_name: 'Speaker 1', segment_count: 3 },
        { id: 'sp-2', label: 'Speaker 2', display_name: 'Speaker 2', segment_count: 1 },
      ];
    case 'api_list_attachments':
      return [];
    case 'api_get_meeting_notes':
      return null;
    case 'api_save_meeting_notes':
      return true;
    case 'api_search_transcripts': {
      const q = ((args?.query as string) ?? '').toLowerCase();
      if (!q) return [];
      return MEETINGS.filter((m) => m.title.toLowerCase().includes(q) || q === 'kernel')
        .slice(0, 5)
        .map((m) => ({ id: m.id, title: m.title, matchContext: `…we never need the feature map explicitly, the ${q} handles it…` }));
    }
    case 'api_list_templates':
      return [];
    // Version / platform probes
    case 'plugin:app|version':
      return '0.2.0-dev';
    // Event system: listen/unlisten return opaque ids; emits are dropped.
    case 'plugin:event|listen':
      return 1;
    case 'plugin:event|unlisten':
    case 'plugin:event|emit':
      return null;
    // Store plugin: pretend an empty store. load → resource id, get → [value, exists].
    case 'plugin:store|load':
      return 1;
    case 'plugin:store|get':
      return [null, false];
    case 'plugin:store|set':
    case 'plugin:store|save':
      return null;
    default:
      // Unhandled commands resolve to null; callers' try/catch and
      // empty-state handling take it from there.
      return null;
  }
}

if (process.env.NODE_ENV === 'development' && typeof window !== 'undefined') {
  const w = window as unknown as Record<string, unknown>;
  if (!w.__TAURI_INTERNALS__) {
    let callbackId = 0;
    w.__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args?: InvokeArgs) => {
        const result = fixture(cmd, args);
        console.debug(`[devBrowserShim] invoke('${cmd}') →`, result);
        return result;
      },
      transformCallback: (callback?: (response: unknown) => void) => {
        const id = ++callbackId;
        (w as Record<string, unknown>)[`_${id}`] = callback ?? (() => {});
        return id;
      },
      metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
    };
    w.__TAURI_OS_PLUGIN_INTERNALS__ = {
      platform: 'macos',
      version: '15.0.0',
      family: 'unix',
      os_type: 'macos',
      arch: 'aarch64',
      exe_extension: '',
      eol: '\n',
    };
    console.info('[devBrowserShim] Tauri bridge not found - installed browser-preview mock with fixture data.');
  }
}

export {};
