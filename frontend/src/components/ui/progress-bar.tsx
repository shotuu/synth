"use client";

/**
 * Thin accent-colored progress bar for long-running local operations
 * (speaker identification, summary generation). Two modes:
 *  - determinate: a real 0-100 percentage (diarization has genuine
 *    per-segment progress).
 *  - indeterminate: a sliding shimmer for phases with no meaningful
 *    percentage (e.g. summary generation has no token-level signal) —
 *    honest about "still working" without faking a number.
 */
export function ProgressBar({
  percent,
  indeterminate = false,
  className = '',
  trackClassName = 'h-1 bg-gray-100',
  barClassName = 'bg-blue-500',
}: {
  percent?: number;
  indeterminate?: boolean;
  className?: string;
  /** Overrides the track's height/background, e.g. "h-2 bg-gray-200". */
  trackClassName?: string;
  /** Overrides the fill's color/gradient, e.g. "bg-gradient-to-r from-blue-500 to-blue-600". */
  barClassName?: string;
}) {
  return (
    <div className={`w-full rounded-full overflow-hidden ${trackClassName} ${className}`}>
      {indeterminate ? (
        <div className={`h-full w-1/3 rounded-full ${barClassName} animate-[progress-indeterminate_1.2s_ease-in-out_infinite]`} />
      ) : (
        <div
          className={`h-full rounded-full transition-[width] duration-300 ease-out ${barClassName}`}
          style={{ width: `${Math.max(0, Math.min(100, percent ?? 0))}%` }}
        />
      )}
    </div>
  );
}
