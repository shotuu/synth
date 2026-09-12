/** Formats a raw byte count as a human-readable string, e.g. "1.5 MB". */
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const exp = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / Math.pow(1024, exp)).toFixed(exp === 0 ? 0 : 1)} ${units[exp]}`;
}

/** Formats a size already expressed in megabytes (model download sizes). */
export function formatSizeMb(sizeMb: number): string {
  if (sizeMb >= 1000) {
    return `${(sizeMb / 1000).toFixed(1)}GB`;
  }
  return `${sizeMb}MB`;
}

/**
 * Formats a transcript timestamp as recording-relative [MM:SS] when
 * audio_start_time is available, falling back to the wall-clock timestamp
 * string for old transcripts that predate that field.
 */
export function formatTranscriptTime(seconds: number | undefined, fallbackTimestamp: string): string {
  if (seconds === undefined) {
    return fallbackTimestamp;
  }
  const totalSecs = Math.floor(seconds);
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `[${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}]`;
}
