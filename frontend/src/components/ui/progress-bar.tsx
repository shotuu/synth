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
}: {
  percent?: number;
  indeterminate?: boolean;
  className?: string;
}) {
  return (
    <div className={`h-1 w-full rounded-full bg-gray-100 overflow-hidden ${className}`}>
      {indeterminate ? (
        <div className="h-full w-1/3 rounded-full bg-blue-500 animate-[progress-indeterminate_1.2s_ease-in-out_infinite]" />
      ) : (
        <div
          className="h-full rounded-full bg-blue-500 transition-[width] duration-300 ease-out"
          style={{ width: `${Math.max(0, Math.min(100, percent ?? 0))}%` }}
        />
      )}
    </div>
  );
}
