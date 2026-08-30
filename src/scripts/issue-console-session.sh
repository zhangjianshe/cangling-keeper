#!/bin/bash
# Issue a cangling-update console session for keeper auto-login.
# Prefers `cangling-update issue-session`; falls back to inserting into sqlite.
set -u
BIN="${1:-}"

if [ -z "$BIN" ]; then
  BIN=$(command -v cangling-update 2>/dev/null || true)
fi
if [ -z "$BIN" ] && [ -x "${HOME:-/root}/update/cangling-update" ]; then
  BIN="${HOME:-/root}/update/cangling-update"
fi
if [ -n "$BIN" ] && command -v readlink >/dev/null 2>&1; then
  r=$(readlink -f "$BIN" 2>/dev/null || true)
  r=${r%" (deleted)"}
  [ -n "$r" ] && BIN=$r
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  printf 'CK_SESSION|ok=0|error=no_binary\n'
  exit 0
fi

if out=$("$BIN" issue-session 2>/dev/null); then
  if printf '%s\n' "$out" | grep -q '^CK_SESSION|'; then
    printf '%s\n' "$out"
    exit 0
  fi
fi

DB="$(dirname "$BIN")/config/cangling.db"
if [ ! -f "$DB" ]; then
  printf 'CK_SESSION|ok=0|error=no_db\n'
  exit 0
fi

if command -v python3 >/dev/null 2>&1; then
  python3 - "$DB" <<'PY'
import sqlite3, sys, uuid
from datetime import datetime, timezone

db = sys.argv[1]
conn = sqlite3.connect(db)
row = conn.execute(
    "SELECT id, username FROM users ORDER BY created_at ASC LIMIT 1"
).fetchone()
if not row:
    print("CK_SESSION|ok=0|error=needs_setup")
    raise SystemExit(0)
token = str(uuid.uuid4())
now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
conn.execute(
    "INSERT INTO sessions (token, user_id, last_seen, created_at) VALUES (?, ?, ?, ?)",
    (token, row[0], now, now),
)
conn.commit()
print("CK_SESSION|ok=1|username=%s|token=%s" % (row[1], token))
PY
  exit 0
fi

printf 'CK_SESSION|ok=0|error=no_python3\n'
exit 0
