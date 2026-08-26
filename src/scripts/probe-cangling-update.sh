#!/bin/bash
# Detect whether cangling-update is installed and which CPU arch this host is.
set -u
HOME="${HOME:-/root}"

arch_raw=$(uname -m 2>/dev/null || echo unknown)
case "$arch_raw" in
  x86_64|amd64) arch=amd64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) arch="unsupported" ;;
esac

installed=0
active=0
binary=""
version=""

if [ -f /etc/systemd/system/cangling-update.service ] \
  || [ -f /lib/systemd/system/cangling-update.service ]; then
  installed=1
fi

if command -v cangling-update >/dev/null 2>&1; then
  installed=1
  binary=$(command -v cangling-update)
  version=$("$binary" version 2>/dev/null | head -n 1 | tr -d '\r' | tr '|' '/')
fi

if [ -z "$binary" ] && [ -x "$HOME/update/cangling-update" ]; then
  binary="$HOME/update/cangling-update"
fi

if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-active --quiet cangling-update 2>/dev/null; then
    active=1
  fi
fi

printf 'CK_PROBE|installed=%s|arch=%s|active=%s|binary=%s|version=%s\n' \
  "$installed" "$arch" "$active" "$binary" "$version"
