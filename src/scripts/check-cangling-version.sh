#!/bin/bash
# $1 = proxy URL, e.g. http://127.0.0.1:7890
# Fetches the latest published version of cangling-update and prints it as
#   CK_VERSION|latest=<version>
# so the caller (Rust) can compare it with the installed version.
set -u
PROXY="${1:-http://127.0.0.1:7890}"
URL="https://soft.cangling.cn:22002/software/a59ff5999a0d4404a257cf7aa16ca10b/latest/version.txt"

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -k --connect-timeout 20 --max-time 60 --proxy "$PROXY" "$URL" 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- --no-check-certificate --timeout=20 \
      -e use_proxy=yes -e "https_proxy=$PROXY" -e "http_proxy=$PROXY" \
      "$URL" 2>/dev/null
  else
    echo "curl/wget not found" >&2
    exit 1
  fi
}

latest=$(fetch | head -n 1 | tr -d '\r\n\t ')
if [ -z "$latest" ]; then
  echo "failed to fetch latest version" >&2
  exit 1
fi

printf 'CK_VERSION|latest=%s\n' "$latest"
