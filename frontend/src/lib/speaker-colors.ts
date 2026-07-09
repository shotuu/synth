/**
 * Consistent per-speaker colors — the app's signature element
 * (PROJECT_BRIEF.md §8). The palette anchors on the Synth violet accent:
 * Speaker 1 *is* the interface accent color, and the other seven hues are
 * tuned to its saturation/lightness on dark surfaces, so speaker identity
 * and the app's one deliberate color read as a single system.
 *
 * Default diarization labels ("Speaker 1", "Speaker 2", …) map to palette
 * order deterministically — Speaker 1 is always violet. Renamed speakers
 * hash their name, so a custom name keeps one stable color everywhere it
 * appears (transcript chips, name tags, summary attendee lists).
 *
 * Mirrored in src-tauri/src/organization/export.rs (HTML export renders on
 * a light page, so it uses light-legible variants of the SAME hue order —
 * change one, change both).
 */

export interface SpeakerColor {
  chip: string; // tailwind classes for the label chip
  dot: string; // tailwind classes for a small color dot
}

const PALETTE: SpeakerColor[] = [
  { chip: 'bg-[#8b6dff]/15 text-[#b3a1ff]', dot: 'bg-[#8b6dff]' }, // violet — the accent
  { chip: 'bg-[#4cc4d9]/15 text-[#8fdceb]', dot: 'bg-[#4cc4d9]' }, // cyan
  { chip: 'bg-[#4ecf9a]/15 text-[#93e6c4]', dot: 'bg-[#4ecf9a]' }, // emerald
  { chip: 'bg-[#e0b04f]/15 text-[#edce8d]', dot: 'bg-[#e0b04f]' }, // amber
  { chip: 'bg-[#ef7f9b]/15 text-[#f7b3c5]', dot: 'bg-[#ef7f9b]' }, // rose
  { chip: 'bg-[#61a6f7]/15 text-[#a0c9fb]', dot: 'bg-[#61a6f7]' }, // sky
  { chip: 'bg-[#a3cc5a]/15 text-[#c8e29b]', dot: 'bg-[#a3cc5a]' }, // lime
  { chip: 'bg-[#eb9a5e]/15 text-[#f3c39e]', dot: 'bg-[#eb9a5e]' }, // orange
];

/** Labels that describe an audio source, not an identified person. */
export function isSourceLabel(speaker: string | undefined): boolean {
  return speaker === 'mic' || speaker === 'system' || !speaker;
}

/** "Speaker 3" → 3; anything else → null. Keep in sync with export.rs. */
function defaultSpeakerNumber(label: string): number | null {
  const match = /^speaker\s+(\d+)$/i.exec(label.trim());
  return match ? parseInt(match[1], 10) : null;
}

export function speakerColor(label: string): SpeakerColor {
  const n = defaultSpeakerNumber(label);
  if (n !== null && n >= 1) {
    return PALETTE[(n - 1) % PALETTE.length];
  }
  let hash = 0;
  for (let i = 0; i < label.length; i++) {
    hash = (hash * 31 + label.charCodeAt(i)) | 0;
  }
  return PALETTE[Math.abs(hash) % PALETTE.length];
}
