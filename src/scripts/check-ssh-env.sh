#!/bin/bash
# Check whether the remote sshd allows TCP forwarding. When it blocks the
# reverse proxy inject (`ssh -N -R`), enable `AllowTcpForwarding yes` and
# restart sshd. Prints a single marker line:
#   CK_SSH_ENV|status=ok|changed=0|allow_tcp_forwarding=yes|message=...
set -u

main_cfg="${SSHD_MAIN:-/etc/ssh/sshd_config}"
dropin_dir="/etc/ssh/sshd_config.d"
dropin="$dropin_dir/00-cangling-tcpforward.conf"

# Locate sshd, which may live outside PATH in non-interactive sessions.
SSHD_BIN=""
for p in /usr/sbin/sshd /usr/bin/sshd /sbin/sshd; do
  if [ -x "$p" ]; then SSHD_BIN="$p"; break; fi
done
if [ -z "$SSHD_BIN" ] && command -v sshd >/dev/null 2>&1; then
  SSHD_BIN="$(command -v sshd)"
fi

effective_value() {
  if [ -n "$SSHD_BIN" ]; then
    "$SSHD_BIN" -T 2>/dev/null | awk 'tolower($1)=="allowtcpforwarding" {print tolower($2); exit}'
  fi
}

current="$(effective_value)"
[ -z "$current" ] && current="unknown"

case "$current" in
  yes|all|remote)
    printf 'CK_SSH_ENV|status=ok|changed=0|allow_tcp_forwarding=%s|message=TCP Forwarding 已允许 (%s)\n' "$current" "$current"
    exit 0
    ;;
esac

# Only root can modify the sshd config and restart the daemon.
if [ "$(id -u 2>/dev/null || echo 0)" != "0" ]; then
  printf 'CK_SSH_ENV|status=error|changed=0|allow_tcp_forwarding=%s|message=需要 root 权限修改 sshd 配置\n' "$current"
  exit 1
fi

changed=0
# Replace every existing AllowTcpForwarding directive (main config and all
# drop-ins) so no lower-priority file can keep it disabled.
for f in "$main_cfg" "$dropin_dir"/*.conf; do
  [ -f "$f" ] || continue
  if grep -Eq '^[[:space:]]*AllowTcpForwarding[[:space:]]' "$f" 2>/dev/null; then
    sed -i 's/^[[:space:]]*AllowTcpForwarding[[:space:]].*/AllowTcpForwarding yes/' "$f"
    changed=1
  fi
done

# No directive anywhere: add a top-priority drop-in.
if [ "$changed" = "0" ]; then
  mkdir -p "$dropin_dir"
  printf 'AllowTcpForwarding yes\n' > "$dropin"
  chmod 600 "$dropin"
  changed=1
fi

# Validate the new configuration before touching the daemon.
if [ -n "$SSHD_BIN" ]; then
  if ! "$SSHD_BIN" -t 2>/dev/null; then
    printf 'CK_SSH_ENV|status=error|changed=1|allow_tcp_forwarding=%s|message=sshd 配置校验失败\n' "$current"
    exit 1
  fi
fi

after="$(effective_value)"
[ -z "$after" ] && after="unknown"

case "$after" in
  yes|all|remote)
    # Restart in the background so this session is not dropped before the
    # result is delivered.
    (
      sleep 1
      if command -v systemctl >/dev/null 2>&1; then
        systemctl restart sshd 2>/dev/null || systemctl restart ssh 2>/dev/null
      elif command -v service >/dev/null 2>&1; then
        service sshd restart 2>/dev/null || service ssh restart 2>/dev/null
      else
        pid="$(cat /var/run/sshd.pid 2>/dev/null || cat /run/sshd.pid 2>/dev/null || echo "")"
        [ -n "$pid" ] && kill -HUP "$pid" 2>/dev/null
      fi
    ) >/dev/null 2>&1 &
    printf 'CK_SSH_ENV|status=ok|changed=1|allow_tcp_forwarding=%s|message=已开启 TCP Forwarding 并重启 sshd\n' "$after"
    exit 0
    ;;
  *)
    printf 'CK_SSH_ENV|status=error|changed=1|allow_tcp_forwarding=%s|message=修改后仍为 %s\n' "$after" "$after"
    exit 1
    ;;
esac
