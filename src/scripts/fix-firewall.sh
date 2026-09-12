#!/bin/bash
# Open the cangling-update listen port in the host firewall so the cluster
# master can reach this node's web console / m2m API over HTTP.
# $1 = cangling-update port (e.g. 80 or 5400)
# $2 = node role (standalone/master/worker)
# $3 = comma-separated hostnames/IPs in the same keeper group
# Prints a single marker line:
#   CK_FIREWALL|status=ok|changed=1|firewall=firewalld|port=5400|message=...
set -u

port="${1:-5400}"
role="${2:-standalone}"
peers="${3:-}"
case "$port" in
  ''|*[!0-9]*) port=5400 ;;
esac
[ "$port" -ge 1 ] 2>/dev/null || port=5400
[ "$port" -le 65535 ] 2>/dev/null || port=5400

firewall=""
changed=0
message=""

# firewalld (RHEL/CentOS/Kylin etc.)
if command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state >/dev/null 2>&1; then
  firewall="firewalld"
  if [ "$(id -u 2>/dev/null || echo 0)" != "0" ]; then
    printf 'CK_FIREWALL|status=error|changed=0|firewall=%s|port=%s|message=需要 root 权限配置防火墙\n' "$firewall" "$port"
    exit 1
  fi

  failed=0
  resolved=0
  direct=0
  routed=0
  unresolved=0

  ensure_port() {
    target_port="$1"
    protocol="$2"
    if ! firewall-cmd --permanent --query-port="${target_port}/${protocol}" >/dev/null 2>&1; then
      if firewall-cmd --permanent --add-port="${target_port}/${protocol}" >/dev/null 2>&1; then
        changed=1
      else
        failed=1
      fi
    fi
  }

  ensure_peer_port() {
    peer_ip="$1"
    target_port="$2"
    protocol="$3"
    rule="rule family=\"ipv4\" source address=\"${peer_ip}/32\" port port=\"${target_port}\" protocol=\"${protocol}\" accept"
    if ! firewall-cmd --permanent --query-rich-rule="$rule" >/dev/null 2>&1; then
      if firewall-cmd --permanent --add-rich-rule="$rule" >/dev/null 2>&1; then
        changed=1
      else
        failed=1
      fi
    fi
  }

  # The web/API port keeps its existing behaviour. K3s ports below are never
  # opened globally; each rule is restricted to a resolved same-group peer.
  ensure_port "$port" tcp

  remaining="$peers"
  while [ -n "$remaining" ]; do
    case "$remaining" in
      *,*) peer="${remaining%%,*}"; remaining="${remaining#*,}" ;;
      *) peer="$remaining"; remaining="" ;;
    esac
    peer="$(printf '%s' "$peer" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
    [ -n "$peer" ] || continue
    if printf '%s\n' "$peer" | grep -Eq '^[0-9]+(\.[0-9]+){3}$'; then
      peer_ip="$peer"
    else
      peer_ip="$(getent ahostsv4 "$peer" 2>/dev/null | awk 'NR==1 {print $1}')"
    fi
    route_info=""
    if [ -n "$peer_ip" ] && command -v ip >/dev/null 2>&1; then
      route_info="$(ip -4 route get "$peer_ip" 2>/dev/null | head -n1)"
    fi
    if [ -z "$peer_ip" ] || [ -z "$route_info" ]; then
      unresolved=$((unresolved + 1))
      continue
    fi
    resolved=$((resolved + 1))
    case " $route_info " in
      *" via "*) routed=$((routed + 1)) ;;
      *) direct=$((direct + 1)) ;;
    esac
    case "$role" in
      master) ensure_peer_port "$peer_ip" 6443 tcp ;;
    esac
    case "$role" in
      master|worker)
        ensure_peer_port "$peer_ip" 8472 udp
        ensure_peer_port "$peer_ip" 10250 tcp
        ensure_peer_port "$peer_ip" 5401 udp
        ;;
    esac
  done

  if [ "$failed" = "1" ]; then
    printf 'CK_FIREWALL|status=error|changed=%s|firewall=%s|port=%s|message=部分组内 K3s 防火墙规则配置失败\n' "$changed" "$firewall" "$port"
    exit 1
  fi
  if [ "$changed" = "1" ] && ! firewall-cmd --reload >/dev/null 2>&1; then
    printf 'CK_FIREWALL|status=error|changed=1|firewall=%s|port=%s|message=规则已写入但 firewalld reload 失败\n' "$firewall" "$port"
    exit 1
  fi
  message="已配置 ${port}/tcp；组内节点解析 ${resolved} 个（同网段 ${direct}、跨网段 ${routed}、不可达 ${unresolved}）"
  case "$role" in
    master) message="$message；按节点 IP 开放 6443/tcp、8472/udp、10250/tcp、5401/udp" ;;
    worker) message="$message；按节点 IP 开放 8472/udp、10250/tcp、5401/udp" ;;
    *) message="$message；当前非集群角色，未增加 K3s 规则" ;;
  esac
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
