#!/usr/bin/env bash
# Launches a packaged app, imports a test tone and takes a screenshot, to prove the build runs.
# Usage: scripts/smoke.sh <path to mouthpiece binary> <output directory>
set -euo pipefail

binary="$1"
out="$(mkdir -p "$2" && cd "$2" && pwd)"
timeout_seconds=120

python3 - "$out/tone.wav" <<'PY'
import math, struct, sys, wave
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(44100)
    w.writeframes(b"".join(struct.pack("<h", int(12000 * math.sin(i * 0.05))) for i in range(88200)))
PY

cat > "$out/script.txt" <<SCRIPT
wait 0.5
import $out/tone.wav
wait 0.5
idle
wait 0.5
action ZoomFit
action PlayPause
wait 1
screenshot $out/screenshot.png
dump $out/state.json
quit
SCRIPT

MOUTHPIECE_SCRIPT="$out/script.txt" "$binary" > "$out/app.log" 2>&1 &
pid=$!
for _ in $(seq 1 "$timeout_seconds"); do
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"
  cat "$out/app.log"
  echo "The app didn't finish within $timeout_seconds seconds" >&2
  exit 1
fi
wait "$pid" || { status=$?; cat "$out/app.log"; echo "The app exited with status $status" >&2; exit 1; }

python3 - "$out/state.json" "$out/screenshot.png" <<'PY'
import json, os, sys
state = json.load(open(sys.argv[1]))
assert len(state["tracks"]) == 1, "the test tone wasn't imported"
assert state["playhead"] > 0.2, "the transport didn't move: %s" % state["playhead"]
assert os.path.getsize(sys.argv[2]) > 10000, "the screenshot is empty"
print("App ran: track imported, transport moved to %.2f s" % state["playhead"])
PY
