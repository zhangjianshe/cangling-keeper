#!/bin/bash
# Detect whether cangling-update is installed and which CPU arch this host is.
set -u
HOME="${HOME:-/root}"
UNIT=/etc/systemd/system/cangling-update.service

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
role=standalone
token_set=0
token=""
master=""

parse_cluster_flags() {
  local text="$1"
  local tok prev=""
  for tok in $text; do
    tok="${tok%\"}"
    tok="${tok#\"}"
    tok="${tok%\'}"
    tok="${tok#\'}"
    case "$tok" in
      --role=*) role="${tok#--role=}" ;;
      --cluster-token=*)
        token="${tok#--cluster-token=}"
        token_set=1
        ;;
      --master=*) master="${tok#--master=}" ;;
      --role) prev=role ;;
      --cluster-token) prev=token ;;
      --master) prev=master ;;
      CANGLING_ROLE=*) role="${tok#CANGLING_ROLE=}" ;;
      CANGLING_CLUSTER_TOKEN=*)
        token="${tok#CANGLING_CLUSTER_TOKEN=}"
        token_set=1
        ;;
      CANGLING_MASTER=*) master="${tok#CANGLING_MASTER=}" ;;
      *)
        case "$prev" in
          role) role="$tok" ;;
          token)
            token="$tok"
            token_set=1
            ;;
          master) master="$tok" ;;
        esac
        prev=""
        ;;
    esac
  done
}

if [ -f "$UNIT" ] \
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
  exec_show=$(systemctl show -p ExecStart --value cangling-update 2>/dev/null || true)
  [ -n "$exec_show" ] && parse_cluster_flags "$exec_show"
  env_show=$(systemctl show -p Environment --value cangling-update 2>/dev/null || true)
  [ -n "$env_show" ] && parse_cluster_flags "$env_show"
fi

if [ -f "$UNIT" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    case "$line" in
      ExecStart=*|Environment=*) parse_cluster_flags "${line#*=}" ;;
    esac
  done < "$UNIT"
fi

case "$role" in
  master|worker|standalone) ;;
  *) role=standalone ;;
esac

token=$(printf '%s' "$token" | tr -d '\r\n')
[ -n "$token" ] && token_set=1

# token is last so the value may contain '=' or '|'.
printf 'CK_PROBE|installed=%s|arch=%s|active=%s|binary=%s|version=%s|role=%s|token_set=%s|master=%s|token=%s\n' \
  "$installed" "$arch" "$active" "$binary" "$version" "$role" "$token_set" "$master" "$token"
