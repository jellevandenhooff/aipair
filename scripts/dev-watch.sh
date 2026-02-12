#!/usr/bin/env bash
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "Usage: $0 <test-repo-path>"
    exit 1
fi

TEST_REPO="$(cd "$1" && pwd)"
AIPAIR_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "==> aipair dev setup"
echo "    aipair root: $AIPAIR_ROOT"
echo "    test repo:   $TEST_REPO"
echo

# 1. Initial build so binary exists
echo "==> Building aipair..."
cargo build --manifest-path "$AIPAIR_ROOT/Cargo.toml"
echo

# 2. Set up .envrc in test repo (idempotent)
ENVRC="$TEST_REPO/.envrc"
BEGIN_MARKER="# --- aipair-dev-begin ---"
END_MARKER="# --- aipair-dev-end ---"

# Remove existing marker block if present
if [ -f "$ENVRC" ]; then
    # Use sed to delete between markers (inclusive)
    sed -i '' "/$BEGIN_MARKER/,/$END_MARKER/d" "$ENVRC"
fi

# Append marker block
cat >> "$ENVRC" <<EOF
$BEGIN_MARKER
export PATH="$AIPAIR_ROOT/target/debug:\$PATH"
$END_MARKER
EOF

echo "==> Updated $ENVRC with aipair PATH"

# Allow direnv if available
if command -v direnv &>/dev/null; then
    direnv allow "$TEST_REPO"
fi
echo

# 3. Run processes; kill all on exit
pids=()
trap 'echo; echo "==> Shutting down..."; kill "${pids[@]}" 2>/dev/null; wait' EXIT INT TERM

# cargo watch — auto-rebuilds on source changes
(cd "$AIPAIR_ROOT" && cargo watch -x build) &
pids+=($!)

# Start aipair serve with auto-port — it writes .aipair/port
rm -f "$TEST_REPO/.aipair/port"
(cd "$TEST_REPO" && "$AIPAIR_ROOT/target/debug/aipair" serve) &
pids+=($!)

# Wait for port file to appear
echo "==> Waiting for aipair server to start..."
for i in $(seq 1 30); do
    if [ -f "$TEST_REPO/.aipair/port" ]; then
        break
    fi
    sleep 0.2
done

if [ ! -f "$TEST_REPO/.aipair/port" ]; then
    echo "ERROR: aipair server did not write port file"
    exit 1
fi

AIPAIR_PORT=$(cat "$TEST_REPO/.aipair/port")
echo "==> aipair server on port $AIPAIR_PORT"
echo

# vite dev server — reads AIPAIR_PORT for proxy target
(cd "$AIPAIR_ROOT/web" && AIPAIR_PORT="$AIPAIR_PORT" npm run dev) &
pids+=($!)

# Wait for any process to exit
wait -n "${pids[@]}" 2>/dev/null || true
