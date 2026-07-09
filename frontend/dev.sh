#!/bin/bash
# Safe dev launcher: clears the two conditions that cause a stuck-loading,
# unresponsive window (see git history around 2026-07-09 for the incident).
#   1. A leftover dev server already bound to port 3118 (Tauri's own
#      `next dev -p 3118` then fails to start, but the webview loads
#      against whatever's already there anyway).
#   2. A `.next` build left in a half-deleted state by a killed/crashed
#      server, so it 200s the HTML shell but 404s the JS/CSS chunks.
# Does NOT touch node_modules or run a production `next build` -- this is
# for everyday dev launches. Use clean_run.sh for a from-scratch rebuild.
set -e

cd "$(dirname "$0")"

LOG_LEVEL=${1:-info}
case $LOG_LEVEL in
    info|debug|trace) export RUST_LOG=$LOG_LEVEL ;;
    *) echo "Invalid log level: $LOG_LEVEL. Valid options: info, debug, trace"; exit 1 ;;
esac

PORT=3118
PORT_PID=$(lsof -nP -iTCP:$PORT -sTCP:LISTEN -t 2>/dev/null || true)
if [ -n "$PORT_PID" ]; then
    # -Fn field-mode output (one value per line) instead of column parsing --
    # this repo's path contains a space ("All Files"), which breaks awk/column splitting.
    PORT_CWD=$(lsof -a -d cwd -p "$PORT_PID" -Fn 2>/dev/null | grep '^n' | cut -c2-)
    if [[ "$PORT_CWD" == "$(pwd)"* ]]; then
        echo "Killing stale dev server on port $PORT (pid $PORT_PID)..."
        kill -TERM "$PORT_PID" 2>/dev/null || true
        sleep 1
    else
        echo "Port $PORT is held by an unrelated process (pid $PORT_PID, cwd: ${PORT_CWD:-unknown})."
        echo "Refusing to kill it -- free the port yourself and re-run."
        exit 1
    fi
fi

# Only our own dev binary, never the separately-installed production app
# at /Applications/meetily.app -- match the resolved (no "/../") repo path.
REPO_ROOT="$(cd .. && pwd)"
STALE_BIN_PID=$(pgrep -f "$REPO_ROOT/target/debug/synth" 2>/dev/null || true)
if [ -n "$STALE_BIN_PID" ]; then
    echo "Closing a leftover app window (pid $STALE_BIN_PID)..."
    kill -TERM "$STALE_BIN_PID" 2>/dev/null || true
    sleep 1
fi

if [ ! -d node_modules ]; then
    echo "node_modules missing, installing dependencies..."
    pnpm install
fi

echo "Clearing .next to guarantee a build that matches current source..."
rm -rf .next

echo "Starting Synth (RUST_LOG=$LOG_LEVEL)..."
pnpm run tauri:dev
