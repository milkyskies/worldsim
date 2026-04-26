#!/usr/bin/env bash
# Run the canonical Tracy perf trace and emit a comparable per-system CSV.
#
# Output: $1/zones-systems.csv — one row per Bevy system span,
#         columns name,total_ms,calls,mean_us, sorted by total_ms desc.
#
# Usage:
#   scripts/perf-snapshot.sh debug/perf/<label>
#
# Requires:
#   - tracy-capture and tracy-csvexport on PATH (see docs/perf_overlay.md)
#   - Tracy v0.13.1 wire protocol
#   - macOS users: CPLUS_INCLUDE_PATH for the from-source Tracy build
#
# Configuration knobs (export to override):
#   SEED   default 42
#   TICKS  default 5000
set -euo pipefail

OUT_DIR="${1:?usage: $0 <out-dir>}"
SEED="${SEED:-42}"
TICKS="${TICKS:-5000}"

mkdir -p "$OUT_DIR"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v tracy-capture >/dev/null 2>&1; then
  echo "tracy-capture not on PATH — see docs/perf_overlay.md for install" >&2
  exit 1
fi
if ! command -v tracy-csvexport >/dev/null 2>&1; then
  echo "tracy-csvexport not on PATH — see docs/perf_overlay.md for install" >&2
  exit 1
fi

if [ "$(uname -s)" = "Darwin" ] && [ -z "${CPLUS_INCLUDE_PATH:-}" ]; then
  export CPLUS_INCLUDE_PATH=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/c++/v1
fi

echo "[perf-snapshot] building --no-default-features --features profile-tracy"
cargo build --release --no-default-features --features profile-tracy

TRACE="$OUT_DIR/run.tracy"
RAW_CSV="$OUT_DIR/zones-raw.csv"
SYSTEMS_CSV="$OUT_DIR/zones-systems.csv"

rm -f "$TRACE" "$RAW_CSV" "$SYSTEMS_CSV"

echo "[perf-snapshot] starting tracy-capture → $TRACE"
tracy-capture -o "$TRACE" >"$OUT_DIR/capture.log" 2>&1 &
CAPTURE_PID=$!
sleep 1

echo "[perf-snapshot] running sim (seed=$SEED ticks=$TICKS)"
./target/release/worldsim --headless --game-defaults --seed "$SEED" --ticks "$TICKS" >"$OUT_DIR/sim.log" 2>&1 || true

wait "$CAPTURE_PID"

echo "[perf-snapshot] exporting csv"
tracy-csvexport "$TRACE" >"$RAW_CSV"

python3 - "$RAW_CSV" "$SYSTEMS_CSV" <<'PY'
import csv, re, sys

raw_path, out_path = sys.argv[1], sys.argv[2]
rows = []
with open(raw_path) as fh:
    reader = csv.reader(fh)
    header = next(reader)
    name_i = header.index("name")
    total_i = header.index("total_ns")
    count_i = header.index("counts")
    mean_i = header.index("mean_ns")
    for row in reader:
        m = re.match(r'^system\{name="([^"]+)"\}', row[name_i])
        if not m:
            continue
        rows.append((m.group(1), int(row[total_i]), int(row[count_i]), float(row[mean_i])))

rows.sort(key=lambda r: -r[1])

with open(out_path, "w") as fh:
    w = csv.writer(fh)
    w.writerow(["system", "total_ms", "calls", "mean_us"])
    for name, total_ns, calls, mean_ns in rows:
        w.writerow([name, f"{total_ns / 1e6:.1f}", calls, f"{mean_ns / 1000:.2f}"])
PY

echo "[perf-snapshot] done"
echo "  trace : $TRACE"
echo "  csv   : $SYSTEMS_CSV"
head -10 "$SYSTEMS_CSV"
