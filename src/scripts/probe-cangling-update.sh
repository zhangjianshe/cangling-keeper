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
port=0

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
      --port=*) port="${tok#--port=}" ;;
      --role) prev=role ;;
      --cluster-token) prev=token ;;
      --master) prev=master ;;
      --port) prev=port ;;
      CANGLING_ROLE=*) role="${tok#CANGLING_ROLE=}" ;;
      CANGLING_CLUSTER_TOKEN=*)
        token="${tok#CANGLING_CLUSTER_TOKEN=}"
        token_set=1
        ;;
      CANGLING_MASTER=*) master="${tok#CANGLING_MASTER=}" ;;
      CANGLING_PORT=*) port="${tok#CANGLING_PORT=}" ;;
      *)
        case "$prev" in
          role) role="$tok" ;;
          token)
            token="$tok"
            token_set=1
            ;;
          master) master="$tok" ;;
          port) port="$tok" ;;
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

resolve_binary() {
  local p="$1"
  [ -n "$p" ] || return 0
  if command -v readlink >/dev/null 2>&1; then
    local r
    r=$(readlink -f "$p" 2>/dev/null || true)
    r=${r%" (deleted)"}
    [ -n "$r" ] && p=$r
  fi
  printf '%s' "$p"
}

if command -v cangling-update >/dev/null 2>&1; then
  installed=1
  binary=$(command -v cangling-update)
fi

if [ -z "$binary" ] && [ -x "$HOME/update/cangling-update" ]; then
  binary="$HOME/update/cangling-update"
fi

parse_exec_start() {
  local text="$1"
  local argv=""
  argv=$(printf '%s' "$text" | sed -n 's/.*argv\[\]=\(.*\)/\1/p')
  if [ -n "$argv" ]; then
    argv=$(printf '%s' "$argv" | sed 's/ ; .*//')
    parse_cluster_flags "$argv"
  else
    parse_cluster_flags "$text"
  fi
}

pid=0
if command -v systemctl >/dev/null 2>&1; then
  if systemctl is-active --quiet cangling-update 2>/dev/null; then
    active=1
  fi
  exec_show=$(systemctl show -p ExecStart --value cangling-update 2>/dev/null || true)
  [ -n "$exec_show" ] && parse_exec_start "$exec_show"
  env_show=$(systemctl show -p Environment --value cangling-update 2>/dev/null || true)
  [ -n "$env_show" ] && parse_cluster_flags "$env_show"
  pid=$(systemctl show -p MainPID --value cangling-update 2>/dev/null || true)
fi

if [ -f "$UNIT" ]; then
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    case "$line" in
      ExecStart=*|Environment=*) parse_cluster_flags "${line#*=}" ;;
    esac
  done < "$UNIT"
fi

pid=$(printf '%s' "${pid:-}" | tr -cd '0-9')
[ -n "$pid" ] || pid=0

# Prefer the running service binary so repo/ is next to the process.
# install-service also installs a symlink at /usr/local/bin/cangling-update;
# dirname of that symlink is the wrong repo location.
if [ "$pid" != 0 ] && [ -r "/proc/$pid/exe" ]; then
  exe=$(readlink -f "/proc/$pid/exe" 2>/dev/null || true)
  exe=${exe%" (deleted)"}
  if [ -n "$exe" ]; then
    binary=$exe
    installed=1
  fi
fi
if [ -n "$binary" ]; then
  binary=$(resolve_binary "$binary")
fi
if [ -n "$binary" ] && [ -x "$binary" ]; then
  version=$("$binary" version 2>/dev/null | head -n 1 | tr -d '\r' | tr '|' '/')
fi

# Running process cmdline is the live listen port (overrides a stale unit).
if [ "$pid" != 0 ] && [ -r "/proc/$pid/cmdline" ]; then
  parse_cluster_flags "$(tr '\0' ' ' < "/proc/$pid/cmdline")"
fi

# Listening socket of that pid is the live HTTP port (overrides a stale --port).
if [ "$pid" != 0 ] && command -v ss >/dev/null 2>&1; then
  listen_line=$(ss -lntp 2>/dev/null | grep -E "pid=$pid[,)]" | head -n 1 || true)
  if [ -n "$listen_line" ]; then
    sock=$(printf '%s' "$listen_line" | awk '{print $4}')
    sock_port="${sock##*:}"
    sock_port=$(printf '%s' "$sock_port" | tr -cd '0-9')
    case "$sock_port" in
      ''|0) ;;
      *) port="$sock_port" ;;
    esac
  fi
fi

case "$role" in
  master|worker|standalone) ;;
  *) role=standalone ;;
esac

token=$(printf '%s' "$token" | tr -d '\r\n')
[ -n "$token" ] && token_set=1

port=$(printf '%s' "$port" | tr -cd '0-9')
case "$port" in
  ''|*[!0-9]*) port=0 ;;
esac
if [ "$port" -gt 65535 ] 2>/dev/null; then
  port=0
fi
# cangling-update default listen port when the unit/cmdline omit --port.
if [ "$port" = 0 ]; then
  port=5400
fi

# token is last so the value may contain '=' or '|'.
printf 'CK_PROBE|installed=%s|arch=%s|active=%s|binary=%s|version=%s|role=%s|token_set=%s|master=%s|port=%s|token=%s\n' \
  "$installed" "$arch" "$active" "$binary" "$version" "$role" "$token_set" "$master" "$port" "$token"
