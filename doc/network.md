# Network topology and internet proxy plan

Date: 2026-08-25  
Scope: give air-restricted host `hn` a usable internet proxy **through this project** (`cangling-keeper`), using this workstation (`dev`) as the egress.

This is a plan, not an implementation log. Measured facts below were taken while the keeper `db-tunnel` was connected (`ssh hn` → `127.0.0.1:2222`).

---

## 1. What we actually have

Three machines sit on **different L3 networks**. `hn` cannot open a TCP session to `dev`. `dev` cannot open a TCP session to `hn` except by jumping through the Windows VM `vmwin`.

```
                         Internet
                             ^
                             |  clash-meta mixed-port :7890
                             |  (HTTP CONNECT + SOCKS5, allow-lan)
                             |
  +--------------------------+---------------------------+
  |  dev  (this workstation)                             |
  |  hostname: dev                                       |
  |  wlp0s20f3  192.168.110.93/24  gw 192.168.110.1      |
  |  vmnet8     192.168.72.1/24    (VMware NAT)          |
  |  ssh alias `hn` → 127.0.0.1:2222                     |
  |                                                      |
  |  cangling-keeper tunnel `db-tunnel` (active):        |
  |    ssh -L 2222:10.141.8.61:22 sshuser@vmwin          |
  +--------------------------+---------------------------+
                             |
                             |  VMware vmnet8
                             v
  +--------------------------+---------------------------+
  |  vmwin  (Windows VM, jump host)                      |
  |  192.168.72.128   SSH :22  as sshuser                |
  |  Has a path into the hn site (VPN / private NIC)     |
  |  ICMP to the VM is filtered; TCP/22 works            |
  +--------------------------+---------------------------+
                             |
                             |  private / VPN
                             v
  +--------------------------+---------------------------+
  |  hn  (Kylin V10 aarch64, k3s master)                 |
  |  hostname: localhost.localdomain                     |
  |  enp3s0  10.141.8.61/24  gw 10.141.8.1               |
  |  DNS 10.141.7.53                                     |
  |  LAN neighbor 10.141.8.62                            |
  |  sshd :22  AllowTcpForwarding remote                 |
  |              GatewayPorts no                         |
  |  k3s v1.30.13  pod CIDR 10.42.0.0/24 (cni0)          |
  +------------------------------------------------------+
```

### Host inventory (measured)

| Role | Name in this repo | Address | How we reach it |
| --- | --- | --- | --- |
| Workstation with internet | `dev` | `192.168.110.93` (wifi), `192.168.72.1` (vmnet8) | local |
| Jump VM | `vmwin` (`/etc/hosts`) | `192.168.72.128:22` | direct from `dev` |
| Target | keeper host `l-hn`, ssh config `Host hn` | `10.141.8.61:22` | **only** via `db-tunnel` → `127.0.0.1:2222` |

Keeper already stores:

- Host `l-hn` → `root@localhost:2222` (certificate)
- Tunnel `db-tunnel` → local `2222` → `10.141.8.61:22` through `sshuser@vmwin:22`

### Paths that do **not** exist

| From | To | Result | Why it matters |
| --- | --- | --- | --- |
| `hn` | `192.168.110.93` / `192.168.72.1` / `192.168.72.128` | ping fail | `hn` cannot dial this workstation or clash directly |
| `dev` | `10.141.8.61` | ping fail; traceroute goes out wifi toward the public net | no direct route into the hn LAN |
| `dev` | `10.141.8.1` | ping **succeeds** (~23 ms, ttl 57) | **different** RFC1918 `10.141.8.1` on the wifi path. Do not treat it as hn's gateway |

Clash on `dev` already binds `*:7890` with `allow-lan: true`. That only helps clients that can route to `dev`. `hn` cannot, so LAN-open clash is not a solution by itself.

---

## 2. What "no internet" means on hn

`hn` is not a hard L3 air-gap. It has a default route via `10.141.8.1` and an internal DNS that resolves public names. Egress is **asymmetric**:

| Destination | Without proxy | Notes |
| --- | --- | --- |
| ICMP `8.8.8.8` / `1.1.1.1` | works | ~180–260 ms |
| `https://www.baidu.com` | 200 | ~80 ms |
| `https://mirrors.aliyun.com` | 301 | domestic mirror |
| `https://registry.npmmirror.com` | 200 | |
| `https://pypi.org` | 200 | slow (~5 s) |
| `https://github.com` | **timeout** (`http_code=000`) | the actual pain |
| `https://crates.io` | 403 | same 403 seen from `dev` on HEAD; CONNECT through the proxy still succeeds |

So the proxy is not "give hn a default route". It is "hairpin GitHub / crates / other blocked or overseas hosts through `dev`'s clash".

Offline packages stay the primary install path (`/opt/cangling-offline`). The proxy is for the leftovers that are not in that bundle.

---

## 3. Direction of SSH forwards (easy to get wrong)

OpenSSH names forwards from **the machine that runs `ssh`**. Keeper currently implements only local forward.

| OpenSSH | Who listens | Traffic exits | Keeper today | hn `sshd` |
| --- | --- | --- | --- | --- |
| `-L` local | `dev` | far side of the SSH hop | **yes** (`direct-tcpip`) | would be **denied** on hn (`AllowTcpForwarding remote`) |
| `-R` remote | `hn` | `dev` | **no** | **allowed** |
| `-D` dynamic SOCKS | the SSH client | far side of the SSH hop | no | wrong direction if run on `dev` |

`db-tunnel` is a `-L` **to `vmwin`**, not to hn. hn only sees a normal SSH login to port 22. That is why `-L` on `vmwin` works even though hn itself refuses local forwards.

To export `dev`'s proxy **onto hn**, traffic must come **back** the existing SSH session: that is `-R`.

hn `sshd` extra constraints:

```
AllowTcpForwarding remote   # -R only
GatewayPorts no             # reverse bind is 127.0.0.1, not 0.0.0.0
PermitTunnel no             # no tun/tap
```

So the proxy socket on hn will be `127.0.0.1:<port>` unless we also change `GatewayPorts` (not required for host-local `curl`/`yum`/`cargo`).

---

## 4. Recommended design

Two layers, both owned by this project:

```
  hn process (curl, cargo, k3s host)
       |
       |  http_proxy / socks5  127.0.0.1:7890
       v
  hn sshd reverse listener 127.0.0.1:7890     ← new keeper tunnel type (-R)
       |
       |  SSH channel, piggybacked on
       |  existing db-tunnel (dev:2222 → vmwin → hn:22)
       v
  dev clash-meta mixed-port 127.0.0.1:7890
       |
       v
  Internet (CN direct, rest via clash rules)
```

**Why clash, not a new proxy binary on hn**

- Already running on `dev`, mixed HTTP + SOCKS5 on one port.
- `hn` is aarch64 Kylin; do not download clash/sing-box onto hn (air-gap rule: no `curl`/`wget` of third-party installers from hn).
- russh 0.63 already supports `tcpip-forward` + `forwarded-tcpip` (remote port forwarding). The missing piece is productized `-R` in keeper, not a new protocol.

**Why not SOCKS `-D` on `dev`**

`-D` on `dev` would SOCKS-proxy *into hn's LAN*. We need the opposite.

**Why not hn-initiated SSH to `dev`**

hn has no route to `192.168.110.93` or `192.168.72.1`. The session must keep being opened from `dev`.

**Why not bind clash on vmwin**

vmwin is a jump box we do not control from this repo. Putting the proxy in keeper keeps connect/disconnect in the same UI as `db-tunnel`.

---

## 5. Manual proof (already done)

With `db-tunnel` connected and clash up:

```sh
# on dev
ssh -N -R 127.0.0.1:17890:127.0.0.1:7890 hn
```

On hn, `ss -tln` showed `127.0.0.1:17890`. Then:

```sh
curl -sI -x http://127.0.0.1:17890 https://github.com          # HTTP/2 200 (was timeout)
curl -sI --socks5-hostname 127.0.0.1:17890 https://github.com # HTTP/2 200
```

Tear the test listener down after use. Production should use port **7890** on hn so env vars match `dev`.

---

## 6. What to build in cangling-keeper

### 6.1 Tunnel direction (core)

Today `Tunnel` is hard-coded as `-L`:

- bind `127.0.0.1:local_port` on `dev`
- `channel_open_direct_tcpip(remote_host, remote_port)` toward the SSH server

Add a direction field:

| Value | OpenSSH | Listen | Dial |
| --- | --- | --- | --- |
| `local` (default, current) | `-L [bind:]local_port:remote_host:remote_port` | `dev` | far side |
| `remote` | `-R [bind:]remote_port:local_host:local_port` | SSH server (`hn`) | `dev` |

Schema sketch (SQLite `tunnels`):

- `direction TEXT NOT NULL DEFAULT 'local'`  (`local` \| `remote`)
- keep `local_port`, `remote_host`, `remote_port`, `ssh_host`, `ssh_port`
- for `remote`, interpret:
  - `remote_port` = listen port **on hn** (7890)
  - `remote_host` = listen bind on hn (`127.0.0.1`; ignore `0.0.0.0` until `GatewayPorts` is yes)
  - `local_port` + a new `local_host` (default `127.0.0.1`) = where `dev` dials (clash)

`parse_ssh_command` must accept `-R` the same way it accepts `-L`.

russh client side:

1. Connect and auth (same as now).
2. `session.tcpip_forward("127.0.0.1", 7890).await` — fails fast if hn refuses or the port is taken.
3. Implement `Handler::server_channel_open_forwarded_tcpip`:
   - `reply.accept().await`
   - `TcpStream::connect(("127.0.0.1", 7890))` on `dev`
   - reuse the existing `pipe()` helper
4. On disconnect: `cancel_tcpip_forward`.

If clash is down, the reverse listener on hn still accepts and then each connection fails. Surface that in the UI ("remote port is up, local proxy 7890 is not").

### 6.2 UI

Tunnel form:

- Direction radio: Local forward / Remote forward (internet into the remote host).
- Placeholder for remote: `ssh -N -R 7890:127.0.0.1:7890 root@127.0.0.1 -p 2222`
- Detail pane should say which side listens.

Preset (optional but worth it): a button on host `l-hn`, **Share local proxy**, that creates/connects:

```
direction    = remote
ssh          = localhost:2222  (the l-hn host record)
listen hn    = 127.0.0.1:7890
dial on dev  = 127.0.0.1:7890   # clash mixed-port, configurable
```

Connect order in the UI:

1. `db-tunnel` must be **active** (otherwise `localhost:2222` is closed).
2. Then connect the reverse proxy tunnel.
3. Disconnect in reverse order.

### 6.3 Optional: env snippet on hn

Do **not** silently rewrite `/etc/environment` on first connect. Offer a one-shot "copy env" / "apply for this shell" that `ssh_execute`s something like:

```sh
# intended for interactive shells and package tools on hn, not for k3s pods
export http_proxy=http://127.0.0.1:7890
export https_proxy=http://127.0.0.1:7890
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890
export ALL_PROXY=socks5h://127.0.0.1:7890
export no_proxy=127.0.0.1,localhost,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,.cangling.cn,hub.cangling.cn
export NO_PROXY="$no_proxy"
```

`no_proxy` must cover k3s (`10.42.0.0/16`, service CIDR if used) so cluster traffic does not hairpin through clash.

Persistent options later, all explicit:

- `/root/.bashrc` snippet
- systemd `drop-in` for a specific unit
- **not** a global `firewalld` transparent redirect

### 6.4 Optional later: keeper-owned proxy

If we do not want to depend on clash:

- keeper listens on `dev` with a tiny HTTP CONNECT + SOCKS5 (or reuse clash only when present)
- reverse-forward that port instead of 7890

Skip this until `-R` works. clash is already the workstation egress.

---

## 7. How to use it, once built

On `dev`:

1. Start clash (mixed-port 7890). Confirm: `curl -sI -x http://127.0.0.1:7890 https://github.com`
2. In keeper, Connect `db-tunnel`. `ssh hn` must work.
3. Connect the new reverse tunnel `hn-proxy` (`-R 7890:127.0.0.1:7890` via `root@127.0.0.1:2222`).

On `hn`:

```sh
export http_proxy=http://127.0.0.1:7890 https_proxy=http://127.0.0.1:7890
curl -sI https://github.com | head
```

Until the feature lands, the same flow is the OpenSSH one-liner in §5 (use 7890 instead of 17890).

---

## 8. k3s / other machines on `10.141.8.0/24`

`GatewayPorts no` means pods and `10.141.8.62` **cannot** use `10.141.8.61:7890`. Only processes on hn that talk to `127.0.0.1:7890`.

If we later need cluster-wide egress:

1. Change hn `sshd` to `GatewayPorts clientspecified` (or `yes`) and reverse-bind `0.0.0.0:7890`.
2. Open `7890/tcp` in firewalld public zone (today: ssh/http/https/6443/10250/…, no 7890).
3. Point pod `HTTP_PROXY` at `10.141.8.61:7890`, still with a tight `NO_PROXY` for cluster CIDRs.

Do not enable `GatewayPorts` unless we explicitly want other hosts on that LAN to share this workstation's proxy.

---

## 9. Implementation sequence

| PR | Work | Done when |
| --- | --- | --- |
| 1 | `Tunnel.direction` + SQLite migrate + `-R` parse + russh `tcpip_forward` path | unit tests for parse; connect reverse to hn, `ss -tln` shows `127.0.0.1:7890` |
| 2 | UI: direction control, parse box, connect-order hint (`db-tunnel` first) | can create `hn-proxy` without hand-editing SQLite |
| 3 | "Share local proxy" preset + clash-port setting (default 7890) | one click after `db-tunnel` is up |
| 4 | Copy-able env snippet / optional `ssh_execute` apply | `curl -sI https://github.com` on hn returns 200 with env set |
| 5 | (optional) `GatewayPorts` / LAN bind for k3s | only if pods need it |

PR 1 is the only one that unblocks the goal. 2–4 are usability. 5 is a policy change on hn.

---

## 10. Risks and rules

- **Do not fetch installers from hn.** Proxy GitHub/crates through `dev`; keep yum/k3s/images on `/opt/cangling-offline`.
- **Clash must be running on `dev`.** Reverse SSH only moves a TCP port; it is not a proxy by itself.
- **Port 7890 on hn** must be free. Nothing listens there today.
- **Do not proxy cluster IPs.** Wrong `no_proxy` will black-hole kube-apiserver / DNS.
- **Secrets stay local.** Jump-host password lives in `config/data.sql`; this document does not repeat it.
- **RFC1918 overlap.** A ping from `dev` to `10.141.8.1` is not proof of connectivity to hn's gateway.

---

## 11. Verification checklist

After PR 1 (or the manual `-R` line):

On `dev`:

- [ ] `ss -tlnp | grep 7890` → clash-meta
- [ ] keeper `db-tunnel` active, `ssh hn hostname` works
- [ ] reverse tunnel active

On `hn`:

- [ ] `ss -tln | grep 7890` → `127.0.0.1:7890`
- [ ] `curl -sI -x http://127.0.0.1:7890 https://github.com` → HTTP/2 200
- [ ] `curl -sI --socks5-hostname 127.0.0.1:7890 https://github.com` → HTTP/2 200
- [ ] `curl -sI https://www.baidu.com` still works **without** proxy (domestic path unchanged)
- [ ] `kubectl get nodes` still works with `no_proxy` set
