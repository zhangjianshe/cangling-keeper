#!/bin/bash
# $1 = install | update
# $2 = amd64 | arm64
# $3 = proxy URL, e.g. http://127.0.0.1:7890
set -eu
ACTION="${1:-}"
ARCH="${2:-}"
PROXY="${3:-http://127.0.0.1:7890}"
HOME="${HOME:-/root}"
DEST_DIR="$HOME/update"
DEST="$DEST_DIR/cangling-update"
BASE="https://soft.cangling.cn:22002/software/a59ff5999a0d4404a257cf7aa16ca10b/latest"

case "$ARCH" in
  amd64) URL="$BASE/cangling-update-linux-amd64" ;;
  arm64) URL="$BASE/cangling-update-linux-arm64" ;;
  *)
    echo "unsupported arch: $ARCH" >&2
    exit 2
    ;;
esac

if [ "$ACTION" != install ] && [ "$ACTION" != update ]; then
  echo "usage: apply-cangling-update.sh install|update amd64|arm64 [proxy]" >&2
  exit 2
fi

mkdir -p "$DEST_DIR"
TMP="$DEST.new.$$"
trap 'rm -f "$TMP"' EXIT

# Linux refuses to truncate a running executable (ETXTBSY / "Text file busy").
# Write a sibling file, then rename over the destination.
replace_bin() {
  local src="$1"
  local dest="$2"
  local dest_real src_real
  dest_real=$(readlink -f "$dest" 2>/dev/null || echo "$dest")
  src_real=$(readlink -f "$src" 2>/dev/null || echo "$src")
  if [ "$src_real" = "$dest_real" ]; then
    return 0
  fi
  local stage="${dest_real}.new.$$"
  cp -f "$src" "$stage"
  chmod +x "$stage"
  mv -f "$stage" "$dest_real"
}

download() {
  echo "CK_APPLY|phase=download|pct=0|msg=正在下载"
  echo "downloading $URL"
  echo "      via   $PROXY"
  echo "      to    $DEST (via $TMP)"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --retry 3 --connect-timeout 20 --max-time 300 -k \
      --proxy "$PROXY" --progress-bar -o "$TMP" "$URL"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$TMP" --no-check-certificate --timeout=20 --progress=bar:force \
      -e use_proxy=yes -e "https_proxy=$PROXY" -e "http_proxy=$PROXY" \
      "$URL"
  else
    echo "CK_APPLY|phase=error|pct=0|msg=未找到 curl/wget" >&2
    echo "curl/wget not found" >&2
    exit 1
  fi
  chmod +x "$TMP"
  if [ ! -s "$TMP" ]; then
    echo "CK_APPLY|phase=error|pct=0|msg=下载文件为空" >&2
    echo "downloaded file is empty" >&2
    exit 1
  fi
  mv -f "$TMP" "$DEST"
  echo "CK_APPLY|phase=download|pct=80|msg=下载完成"
  echo "download ok ($(wc -c < "$DEST") bytes)"
}

download

if [ "$ACTION" = install ]; then
  echo "CK_APPLY|phase=install|pct=85|msg=正在安装服务"
  echo "installing service: $DEST --port=80 install-service"
  (cd "$DEST_DIR" && ./cangling-update --port=80 install-service)
  echo "CK_APPLY|phase=done|pct=100|msg=安装完成"
  echo "CK_APPLY_OK install"
  exit 0
fi

# Installed: overlay the live binary if it is not already ~/update/cangling-update,
# then restart the systemd service.
LIVE=""
if command -v cangling-update >/dev/null 2>&1; then
  LIVE=$(command -v cangling-update)
fi
if [ -n "$LIVE" ]; then
  echo "CK_APPLY|phase=replace|pct=90|msg=正在替换程序"
  echo "replacing live binary $LIVE"
  replace_bin "$DEST" "$LIVE"
  echo "CK_APPLY|phase=restart|pct=95|msg=正在重启服务"
  echo "restarting service"
  cangling-update restart
else
  echo "CK_APPLY|phase=restart|pct=95|msg=正在重启服务"
  echo "restarting via $DEST"
  "$DEST" restart
fi
echo "CK_APPLY|phase=done|pct=100|msg=更新完成"
echo "CK_APPLY_OK update"
