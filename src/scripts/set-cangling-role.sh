#!/bin/bash
# Re-register the cangling-update systemd unit as standalone / master / worker.
# $1 = standalone | master | worker
# $2 = cluster token (required for master/worker unless already in the unit)
# $3 = master URL (optional, worker only; empty = UDP discovery)
set -eu
ROLE="${1:-}"
TOKEN="${2:-}"
MASTER="${3:-}"
HOME="${HOME:-/root}"
UNIT=/etc/systemd/system/cangling-update.service

case "$ROLE" in
  standalone|master|worker) ;;
  *)
    echo "usage: set-cangling-role.sh standalone|master|worker [token] [master]" >&2
    exit 2
    ;;
esac

as_root() {
  if [ "$(id -u 2>/dev/null || echo 1)" = "0" ]; then
    "$@"
  else
    sudo "$@"
  fi
}

quote_arg() {
  local s="$1"
  case "$s" in
    *[!A-Za-z0-9/.:@_+=-]*)
      s=${s//\\/\\\\}
      s=${s//\"/\\\"}
      printf '"%s"' "$s"
      ;;
    *) printf '%s' "$s" ;;
  esac
}

bind="0.0.0.0"
port="5400"
data_dir=""
exe=""
workdir=""
existing_token=""

parse_exec() {
  local text="$1"
  local tok prev=""
  for tok in $text; do
    tok="${tok%\"}"
    tok="${tok#\"}"
    tok="${tok%\'}"
    tok="${tok#\'}"
    case "$tok" in
      --bind=*) bind="${tok#--bind=}" ;;
      --port=*) port="${tok#--port=}" ;;
      --data-dir=*) data_dir="${tok#--data-dir=}" ;;
      --cluster-token=*) existing_token="${tok#--cluster-token=}" ;;
      --bind) prev=bind ;;
      --port) prev=port ;;
      --data-dir) prev=data_dir ;;
      --cluster-token) prev=token ;;
      --master) prev="" ;;
      --role|--role=*|--discovery-port|--discovery-port=*) prev="" ;;
      CANGLING_CLUSTER_TOKEN=*) existing_token="${tok#CANGLING_CLUSTER_TOKEN=}" ;;
      CANGLING_BIND=*) bind="${tok#CANGLING_BIND=}" ;;
      CANGLING_PORT=*) port="${tok#CANGLING_PORT=}" ;;
      *)
        case "$prev" in
          bind) bind="$tok" ;;
          port) port="$tok" ;;
          data_dir) data_dir="$tok" ;;
          token) existing_token="$tok" ;;
          "")
            case "$tok" in
              --*) ;;
              /*)
                if [ -z "$exe" ] && [ -x "$tok" ]; then
                  exe="$tok"
                fi
                ;;
            esac
            ;;
        esac
        prev=""
        ;;
    esac
  done
}

if [ ! -f "$UNIT" ]; then
  echo "cangling-update 服务未安装，请先安装更新程序" >&2
  exit 2
fi

while IFS= read -r line || [ -n "$line" ]; do
  line="${line%$'\r'}"
  case "$line" in
    ExecStart=*|Environment=*) parse_exec "${line#*=}" ;;
    WorkingDirectory=*)
      workdir="${line#WorkingDirectory=}"
      workdir="${workdir%\"}"
      workdir="${workdir#\"}"
      ;;
  esac
done < "$UNIT"

if [ -z "$exe" ]; then
  if command -v cangling-update >/dev/null 2>&1; then
    exe=$(command -v cangling-update)
  elif [ -x "$HOME/update/cangling-update" ]; then
    exe="$HOME/update/cangling-update"
  fi
fi

if [ -z "$exe" ] || [ ! -x "$exe" ]; then
  echo "找不到 cangling-update 可执行文件" >&2
  exit 2
fi

if [ -z "$workdir" ]; then
  workdir=$(dirname "$exe")
fi

if [ -z "$TOKEN" ]; then
  TOKEN="$existing_token"
fi

if [ "$ROLE" = "master" ] || [ "$ROLE" = "worker" ]; then
  if [ -z "$TOKEN" ]; then
    echo "集群角色 $ROLE 需要 cluster-token" >&2
    exit 2
  fi
fi

if [ "$ROLE" = "standalone" ]; then
  TOKEN=""
  MASTER=""
elif [ "$ROLE" = "master" ]; then
  MASTER=""
fi

exec_line="$(quote_arg "$exe") --bind $(quote_arg "$bind") --port $(quote_arg "$port")"
if [ -n "$data_dir" ]; then
  exec_line="$exec_line --data-dir $(quote_arg "$data_dir")"
fi
if [ "$ROLE" != "standalone" ]; then
  exec_line="$exec_line --role $(quote_arg "$ROLE") --cluster-token $(quote_arg "$TOKEN")"
  if [ -n "$MASTER" ]; then
    exec_line="$exec_line --master $(quote_arg "$MASTER")"
  fi
else
  exec_line="$exec_line --role standalone"
fi

unit_body=$(cat <<EOF
[Unit]
Description=Cangling Update docker-compose host updater
Documentation=file:$exe
After=network-online.target docker.service docker.socket
Wants=network-online.target docker.service

[Service]
Type=simple
WorkingDirectory=$(quote_arg "$workdir")
ExecStart=$exec_line
Restart=on-failure
RestartSec=3
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF
)

echo "registering cangling-update as $ROLE"
echo "  ExecStart=$exec_line"

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
printf '%s\n' "$unit_body" > "$tmp"
as_root cp "$tmp" "$UNIT"
as_root chmod 644 "$UNIT"
as_root systemctl daemon-reload
as_root systemctl enable cangling-update >/dev/null
as_root systemctl restart cangling-update

active=0
if as_root systemctl is-active --quiet cangling-update; then
  active=1
fi

token_set=0
[ -n "$TOKEN" ] && token_set=1

echo "CK_ROLE|role=$ROLE|active=$active|token_set=$token_set|master=$MASTER"
if [ "$active" != "1" ]; then
  echo "service failed to start" >&2
  as_root systemctl --no-pager --full status cangling-update || true
  exit 1
fi
echo "registered ok ($ROLE)"
