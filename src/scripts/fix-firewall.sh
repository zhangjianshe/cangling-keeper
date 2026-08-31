#!/bin/bash
# Open the cangling-update listen port in the host firewall so the cluster
# master can reach this node's web console / m2m API over HTTP.
# $1 = port (e.g. 80 or 5400)
# Prints a single marker line:
#   CK_FIREWALL|status=ok|changed=1|firewall=firewalld|port=80|message=...
set -u

port="${1:-80}"
case "$port" in
  ''|*[!0-9]*) port=80 ;;
esac
[ "$port" -ge 1 ] 2>/dev/null || port=80
[ "$port" -le 65535 ] 2>/dev/null || port=80

firewall=""
changed=0
message=""

# firewalld (RHEL/CentOS/Kylin etc.)
if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
  firewall="firewalld"
  if firewall-cmd --query-port="${port}/tcp" >/dev/null 2>&1; then
    message="端口 ${port}/tcp 已开放"
  else
    if [ "$(id -u 2>/dev/null || echo 0)" != "0" ]; then
      printf 'CK_FIREWALL|status=error|changed=0|firewall=%s|port=%s|message=需要 root 权限开放端口\n' "$firewall" "$port"
      exit 1
    fi
    firewall-cmd --permanent --add-port="${port}/tcp" >/dev/null 2>&1 || true
    firewall-cmd --reload >/dev/null 2>&1 || true
    changed=1
    message="已开放端口 ${port}/tcp"
  fi
  printf 'CK_FIREWALL|status=ok|changed=%s|firewall=%s|port=%s|message=%s\n' "$changed" "$firewall" "$port" "$message"
  exit 0
fi

# iptables (fallback when no firewalld is running)
if command -v iptables >/dev/null 2>&1; then
  firewall="iptables"
  if iptables -C INPUT -p tcp --dport "$port" -j ACCEPT >/dev/null 2>&1; then
    message="端口 ${port}/tcp 已开放"
  else
    if [ "$(id -u 2>/dev/null || echo 0)" != "0" ]; then
      printf 'CK_FIREWALL|status=error|changed=0|firewall=%s|port=%s|message=需要 root 权限开放端口\n' "$firewall" "$port"
      exit 1
    fi
    iptables -I INPUT -p tcp --dport "$port" -j ACCEPT >/dev/null 2>&1 || true
    changed=1
    message="已开放端口 ${port}/tcp（iptables，未持久化）"
  fi
  printf 'CK_FIREWALL|status=ok|changed=%s|firewall=%s|port=%s|message=%s\n' "$changed" "$firewall" "$port" "$message"
  exit 0
fi

# ufw (Ubuntu/Debian)
if command -v ufw >/dev/null 2>&1; then
  firewall="ufw"
  if ufw status 2>/dev/null | grep -q "^${port}/tcp"; then
    message="端口 ${port}/tcp 已开放"
  else
    if [ "$(id -u 2>/dev/null || echo 0)" != "0" ]; then
      printf 'CK_FIREWALL|status=error|changed=0|firewall=%s|port=%s|message=需要 root 权限开放端口\n' "$firewall" "$port"
      exit 1
    fi
    ufw allow "${port}/tcp" >/dev/null 2>&1 || true
    changed=1
    message="已开放端口 ${port}/tcp"
  fi
  printf 'CK_FIREWALL|status=ok|changed=%s|firewall=%s|port=%s|message=%s\n' "$changed" "$firewall" "$port" "$message"
  exit 0
fi

printf 'CK_FIREWALL|status=skip|changed=0|firewall=none|port=%s|message=未检测到防火墙，无需开放端口\n' "$port"
