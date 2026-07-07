/**
 * Consistent per-speaker colors (PROJECT_BRIEF.md §8's signature element).
 * A speaker label always hashes to the same palette entry, so the color
 * stays stable across the transcript, name chips, and re-renders — and
 * survives renames within a session gracefully (a renamed speaker gets a
 * color from their new name, still consistent everywhere it appears).
 */

export interface SpeakerColor {
  chip: string; // tailwind classes for the label chip
  dot: string; // tailwind classes for a small color dot
}

const PALETTE: SpeakerColor[] = [
  { chip: 'bg-blue-100 text-blue-800', dot: 'bg-blue-500' },
  { chip: 'bg-emerald-100 text-emerald-800', dot: 'bg-emerald-500' },
  { chip: 'bg-amber-100 text-amber-800', dot: 'bg-amber-500' },
  { chip: 'bg-purple-100 text-purple-800', dot: 'bg-purple-500' },
  { chip: 'bg-rose-100 text-rose-800', dot: 'bg-rose-500' },
  { chip: 'bg-cyan-100 text-cyan-800', dot: 'bg-cyan-500' },
  { chip: 'bg-lime-100 text-lime-800', dot: 'bg-lime-600' },
  { chip: 'bg-orange-100 text-orange-800', dot: 'bg-orange-500' },
];

/** Labels that describe an audio source, not an identified person. */
export function isSourceLabel(speaker: string | undefined): boolean {
  return speaker === 'mic' || speaker === 'system' || !speaker;
}

export function speakerColor(label: string): SpeakerColor {
  let hash = 0;
  for (let i = 0; i < label.length; i++) {
    hash = (hash * 31 + label.charCodeAt(i)) | 0;
  }
  return PALETTE[Math.abs(hash) % PALETTE.length];
}
