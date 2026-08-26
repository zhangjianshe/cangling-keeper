// `withGlobalTauri: true` in tauri.conf.json injects the API at window.__TAURI__.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  section: "hosts", // "hosts" | "tunnels" | "certificates" | "proxy"
  hosts: [],
  tunnels: [], // [{...tunnel, active}]
  certificates: [],
  selectedHostId: null,
  selectedTunnelId: null,
  selectedCertId: null,
  editingHostId: null,
  editingTunnelId: null,
  termId: null,
  connectedHostId: null,
  proxy: null, // shared ProxyStatus for every panel
  injections: {}, // hostId -> { active, remotePort, localEndpoint }
  updateProbe: null,
  updateBusy: false,
  appUpdate: null,
  appUpdateBusy: false,
};

const $ = (sel) => document.querySelector(sel);

const addBtnEl = $("#add-btn");
const hostListEl = $("#host-list");
const tunnelListEl = $("#tunnel-list");
const certListEl = $("#cert-list");
const emptyStateEl = $("#empty-state");
const emptyTitleEl = $("#empty-title");
const emptySubEl = $("#empty-sub");
const hostViewEl = $("#host-view");
const tunnelViewEl = $("#tunnel-view");
const certViewEl = $("#cert-view");
const proxyViewEl = $("#proxy-view");
const proxySidebarEl = $("#proxy-sidebar");

const hostNameEl = $("#host-name");
const hostConnEl = $("#host-conn");
const termToggleBtnEl = $("#term-toggle-btn");
const termStatusEl = $("#term-status");
const injectBtnEl = $("#inject-proxy-btn");
const injectStatusEl = $("#inject-status");
const updateBtnEl = $("#btn-cangling-update");
const appUpdateBtnEl = $("#app-update-btn");
const terminalEl = $("#terminal");

const tunnelNameEl = $("#tunnel-name");
const tunnelStatusEl = $("#tunnel-status");
const tLocalEl = $("#t-local");
const tRemoteEl = $("#t-remote");
const tSshEl = $("#t-ssh");
const tAuthEl = $("#t-auth");
const toggleTunnelBtnEl = $("#toggle-tunnel-btn");

const certNameEl = $("#cert-name");
const cKeypathEl = $("#c-keypath");
const cPubkeyEl = $("#c-pubkey");

const hostModalEl = $("#host-modal");
const hostFormEl = $("#host-form");
const hostModalTitleEl = $("#host-modal-title");
const hostAuthPasswordEl = $("#host-auth-password");
const hostAuthCertEl = $("#host-auth-cert");

const tunnelModalEl = $("#tunnel-modal");
const tunnelFormEl = $("#tunnel-form");
const tunnelModalTitleEl = $("#tunnel-modal-title");
const sshCmdInputEl = $("#ssh-cmd-input");
const tunnelAuthPasswordEl = $("#tunnel-auth-password");
const tunnelAuthCertEl = $("#tunnel-auth-cert");

const certModalEl = $("#cert-modal");
const certFormEl = $("#cert-form");

// ---- xterm.js terminal ------------------------------------------------------

let term = null;
let fitAddon = null;

function initTerminal() {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: '"Cascadia Mono", "SF Mono", Menlo, Consolas, monospace',
    scrollback: 10000,
    theme: {
      background: "#0d1117",
      foreground: "#e6edf3",
      cursor: "#2dd4bf",
      selectionBackground: "#264f78",
    },
  });

  fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);
  term.open(terminalEl);

  // Auto-fit whenever the terminal container changes size (first show,
  // window resize, layout changes). Avoids the initial shrink-to-zero issue.
  const resizeObserver = new ResizeObserver(() => {
    if (fitAddon && terminalEl.offsetWidth > 0 && terminalEl.offsetHeight > 0) {
      fitAddon.fit();
    }
  });
  resizeObserver.observe(terminalEl);

  term.onData((data) => {
    if (state.termId) {
      invoke("terminal_input", { termId: state.termId, data }).catch(() => {});
    }
  });

  term.onResize(({ cols, rows }) => {
    if (state.termId) {
      invoke("terminal_resize", { termId: state.termId, cols, rows }).catch(() => {});
    }
  });
}

function updateTerminalUI() {
  const connected = !!state.termId;
  termToggleBtnEl.textContent = connected ? "断开连接" : "连接";
  termToggleBtnEl.className = "btn " + (connected ? "danger" : "primary");
  termStatusEl.textContent = connected ? "Connected" : "Disconnected";
  termStatusEl.className = "status " + (connected ? "on" : "off");
  updateHostActionsUI();
  renderHostList();
  if (connected) {
    probeCanglingUpdate();
  } else {
    state.updateProbe = null;
  }
}

async function disconnectTerminal() {
  const id = state.termId;
  state.termId = null;
  state.connectedHostId = null;
  if (id) {
    await invoke("terminal_close", { termId: id }).catch(() => {});
  }
  term.reset();
  updateTerminalUI();
}

async function toggleTerminal() {
  if (state.termId) {
    await disconnectTerminal();
    return;
  }

  const host = hostById(state.selectedHostId);
  if (!host) return;

  termToggleBtnEl.disabled = true;
  termToggleBtnEl.textContent = "连接中…";
  try {
    await autoInjectIfPossible(host);
    // Ensure the terminal is sized correctly right before connecting.
    if (fitAddon) fitAddon.fit();
    const termId = await invoke("start_terminal", {
      hostId: host.id,
      cols: term.cols,
      rows: term.rows,
    });
    state.termId = termId;
    state.connectedHostId = host.id;
    updateTerminalUI();
    term.focus();
  } catch (err) {
    alert(`Error: ${err}`);
    updateTerminalUI();
  } finally {
    termToggleBtnEl.disabled = false;
  }
}

function hostInjectRemotePort(host) {
  const n = host && Number(host.inject_remote_port);
  return n > 0 ? n : 7890;
}

function updateInjectUI() {
  const hostId = state.selectedHostId;
  const host = hostById(hostId);
  const inj = hostId ? state.injections[hostId] : null;
  const active = !!(inj && inj.active);
  const proxy = currentProxy();
  const remotePort = hostInjectRemotePort(host);

  if (active) {
    injectBtnEl.textContent = "关闭代理";
    injectBtnEl.className = "btn danger";
    injectBtnEl.disabled = false;
    injectBtnEl.title = `ssh -N -R ${inj.remotePort}:${inj.localEndpoint} — 点击关闭`;
    injectStatusEl.textContent = `已注入 ${inj.remotePort} → ${inj.localEndpoint}`;
    injectStatusEl.className = "status on";
    return;
  }

  injectBtnEl.textContent = "注入代理";
  injectBtnEl.className = "btn";
  injectBtnEl.disabled = false;
  injectBtnEl.title = proxy
    ? `ssh -N -R ${remotePort}:${proxy.endpoint}`
    : `连接或点击后检测本地代理，注入到远端 ${remotePort}`;
  injectStatusEl.textContent = proxy ? "未注入（连接时自动注入）" : "连接时将检测并注入";
  injectStatusEl.className = "status off";
}

async function autoInjectIfPossible(host) {
  if (!host || hostInjected(host.id)) return;
  injectBtnEl.disabled = true;
  injectBtnEl.textContent = "检测注入…";
  try {
    const info = await invoke("inject_proxy", { hostId: host.id });
    state.injections[host.id] = info;
  } catch (_) {
    // Connect still proceeds if no local proxy is found.
  } finally {
    updateInjectUI();
  }
}

async function loadInjections() {
  const list = await invoke("list_proxy_injections");
  state.injections = {};
  for (const item of list) {
    state.injections[item.hostId] = item;
  }
  updateInjectUI();
}

async function toggleInject() {
  const host = hostById(state.selectedHostId);
  if (!host) return;
  const inj = state.injections[host.id];
  injectBtnEl.disabled = true;
  try {
    if (inj && inj.active) {
      injectBtnEl.textContent = "断开中…";
      await invoke("uninject_proxy", { hostId: host.id });
      state.injections[host.id] = {
        active: false,
        remotePort: hostInjectRemotePort(host),
        localEndpoint: "",
      };
    } else {
      injectBtnEl.textContent = "检测注入…";
      const info = await invoke("inject_proxy", { hostId: host.id });
      state.injections[host.id] = info;
    }
  } catch (err) {
    alert(`Error: ${err}`);
  } finally {
    updateInjectUI();
    if (state.termId) probeCanglingUpdate();
  }
}

function hostInjected(hostId) {
  const inj = hostId ? state.injections[hostId] : null;
  return !!(inj && inj.active);
}

function updateUpdateBtnAlign() {
  updateBtnEl.classList.toggle("align-left", !currentProxy());
}

function setUpdateButton({ disabled = false, text = "更新程序", title = "", cls = "", hidden = false } = {}) {
  updateBtnEl.disabled = disabled;
  updateBtnEl.textContent = text;
  updateBtnEl.title = title;
  updateBtnEl.classList.remove("primary", "up-to-date", "error", "hidden");
  if (cls) updateBtnEl.classList.add(cls);
  if (hidden) updateBtnEl.classList.add("hidden");
  updateUpdateBtnAlign();
}

function updateHostActionsUI() {
  const connected = !!state.termId;
  if (!connected) {
    setUpdateButton({ disabled: true, hidden: true });
    return;
  }

  if (state.updateBusy) {
    return;
  }

  if (!hostInjected(state.selectedHostId)) {
    setUpdateButton({ disabled: true, title: "需要先注入代理" });
    return;
  }

  const p = state.updateProbe;
  if (!p) {
    setUpdateButton({ disabled: true, text: "检测中…", title: "正在检查 cangling-update…" });
    return;
  }

  if (!p.supported) {
    setUpdateButton({ disabled: true, title: `不支持的架构：${p.arch}` });
    return;
  }

  if (p.installed) {
    const ver = p.version ? ` ${p.version}` : "";
    const run = p.active ? "运行中" : "已安装";
    if (p.versionError) {
      setUpdateButton({
        disabled: false,
        text: "重新检测",
        title: `${run}${ver} · ${p.arch} · 获取最新版本失败：${p.versionError}`,
        cls: "error",
      });
    } else if (p.updateAvailable) {
      setUpdateButton({
        disabled: false,
        text: `更新到 ${p.latest}`,
        title: `${run}${ver} · ${p.arch} · 服务器最新 ${p.latest} · 点击后经代理更新并重启`,
        cls: "primary",
      });
    } else {
      setUpdateButton({
        disabled: true,
        text: "已是最新版本",
        title: `${run}${ver} · ${p.arch} · 已是最新版本（服务器 ${p.latest || "—"}）`,
        cls: "up-to-date",
      });
    }
  } else {
    setUpdateButton({
      disabled: false,
      text: "安装更新程序",
      title: `未安装 · ${p.arch} · 点击后经代理下载并 install-service`,
      cls: "primary",
    });
  }
}

async function probeCanglingUpdate() {
  const hostId = state.selectedHostId;
  if (!state.termId || !hostId) return;
  if (!hostInjected(hostId)) {
    state.updateProbe = null;
    updateHostActionsUI();
    return;
  }
  if (state.updateBusy) return;

  setUpdateButton({ disabled: true, text: "检测中…", title: "正在检查 cangling-update…" });
  try {
    state.updateProbe = await invoke("probe_cangling_update", { hostId });
  } catch (err) {
    state.updateProbe = null;
    setUpdateButton({ disabled: false, text: "重新检测", title: String(err), cls: "error" });
    return;
  }
  updateHostActionsUI();
}

function writeActionLog(title, text) {
  if (!term) return;
  const body = (text || "").replace(/\n/g, "\r\n");
  term.write(`\r\n\x1b[90m[${title}]\x1b[0m\r\n${body}\r\n`);
}

async function applyCanglingUpdate({ busy, status }) {
  const hostId = state.selectedHostId;
  state.updateBusy = true;
  setUpdateButton({ disabled: true, text: busy, title: status });
  try {
    const result = await invoke("run_cangling_update", { hostId });
    writeActionLog(
      result.action === "install" ? "安装更新程序" : "更新程序",
      [result.stdout, result.stderr].filter(Boolean).join("\n")
    );
  } catch (err) {
    writeActionLog("更新程序失败", String(err));
    alert(`Error: ${err}`);
  } finally {
    state.updateBusy = false;
    await probeCanglingUpdate();
  }
}

async function onCanglingUpdateClick() {
  const hostId = state.selectedHostId;
  if (!hostId || !state.termId) return;
  if (state.updateBusy) return;
  if (!hostInjected(hostId)) {
    alert("请先注入代理");
    return;
  }

  // Refresh the probe so the install/update decision uses the current version.
  await probeCanglingUpdate();
  const p = state.updateProbe;
  if (!p) return;

  if (!p.supported) {
    alert(`不支持的架构：${p.arch}`);
    return;
  }

  if (!p.installed) {
    await applyCanglingUpdate({
      busy: "安装中…",
      status: "正在下载并安装 cangling-update…",
    });
    return;
  }

  // Service already installed: compare with the server's latest version.
  if (p.versionError) {
    alert(`获取最新版本失败：${p.versionError}`);
    return;
  }
  if (!p.updateAvailable) {
    updateHostActionsUI();
    return;
  }

  await applyCanglingUpdate({
    busy: "更新中…",
    status: `正在下载并更新到 ${p.latest}…`,
  });
}

// ---- app self-update -------------------------------------------------------

function renderAppUpdate() {
  const s = state.appUpdate;
  if (!s) {
    appUpdateBtnEl.textContent = "检查更新";
    appUpdateBtnEl.className = "btn app-update-btn";
    appUpdateBtnEl.disabled = false;
    appUpdateBtnEl.title = "检查应用更新";
    appUpdateBtnEl.classList.remove("hidden");
    return;
  }
  if (s.error) {
    appUpdateBtnEl.textContent = "更新检查失败";
    appUpdateBtnEl.className = "btn app-update-btn error";
    appUpdateBtnEl.disabled = false;
    appUpdateBtnEl.title = `${s.error}（当前 ${s.current}）· 点击重试`;
  } else if (s.updateAvailable) {
    appUpdateBtnEl.textContent = `更新到 ${s.latest}`;
    appUpdateBtnEl.className = "btn app-update-btn primary";
    appUpdateBtnEl.disabled = false;
    appUpdateBtnEl.title = `当前 ${s.current} · 服务器最新 ${s.latest} · 点击更新`;
  } else {
    appUpdateBtnEl.textContent = "已是最新";
    appUpdateBtnEl.className = "btn app-update-btn up-to-date";
    appUpdateBtnEl.disabled = true;
    appUpdateBtnEl.title = `当前 ${s.current} 已是最新版本`;
  }
  appUpdateBtnEl.classList.remove("hidden");
}

async function checkAppUpdate() {
  appUpdateBtnEl.disabled = true;
  appUpdateBtnEl.textContent = "检查中…";
  appUpdateBtnEl.classList.remove("hidden");
  try {
    state.appUpdate = await invoke("check_app_update");
  } catch (err) {
    state.appUpdate = {
      current: "",
      latest: "",
      updateAvailable: false,
      error: String(err),
    };
  }
  renderAppUpdate();
}

async function onAppUpdateClick() {
  if (state.appUpdateBusy) return;
  const s = state.appUpdate;
  if (!s || !s.updateAvailable) {
    await checkAppUpdate();
    return;
  }
  if (!confirm(`发现新版本 ${s.latest}（当前 ${s.current}），现在下载并更新？`)) {
    return;
  }
  state.appUpdateBusy = true;
  appUpdateBtnEl.disabled = true;
  appUpdateBtnEl.textContent = "下载更新中…";
  try {
    await invoke("apply_app_update");
  } catch (err) {
    state.appUpdateBusy = false;
    alert(`更新失败: ${err}`);
    renderAppUpdate();
  }
}

// ---- helpers ----------------------------------------------------------------

function hostById(id) {
  return state.hosts.find((h) => h.id === id);
}

function tunnelById(id) {
  return state.tunnels.find((t) => t.id === id);
}

function certById(id) {
  return state.certificates.find((c) => c.id === id);
}

function certNameById(id) {
  const cert = certById(id);
  return cert ? cert.name : "Unknown";
}

// ---- data loading -----------------------------------------------------------

async function loadHosts() {
  state.hosts = await invoke("list_hosts");
  renderHostList();
}

async function loadTunnels() {
  state.tunnels = await invoke("list_tunnels");
  renderTunnelList();
}

async function loadCertificates() {
  state.certificates = await invoke("list_certificates");
  fillCertSelects();
  renderCertList();
}

async function loadProxy() {
  state.proxy = await invoke("get_proxy_status");
  renderProxy();
}

function proxyIsUsable(p) {
  return !!(p && p.enabled && p.reachable && (p.http || p.socks5));
}

/** Shared proxy for Host / Tunnel / other panels. Null if not usable. */
function currentProxy() {
  return proxyIsUsable(state.proxy) ? state.proxy : null;
}

function applyProxyStatus(status) {
  state.proxy = status;
  renderProxy();
}

function renderProxy() {
  const p = state.proxy || {
    mode: "",
    endpoint: "",
    enabled: false,
    running: false,
    reachable: false,
    http: false,
    socks5: false,
    message: "",
    lastCheckedAt: 0,
    host: "",
    port: 0,
  };
  const usable = proxyIsUsable(p);
  const mode = p.mode || "local";

  document.querySelectorAll('input[name="proxy_mode"]').forEach((r) => {
    r.checked = r.value === mode || (mode === "" && r.value === "local");
  });
  $("#proxy-option-local").classList.toggle("selected", mode === "local" || mode === "");
  $("#proxy-option-existing").classList.toggle("selected", mode === "existing");

  if (p.port) {
    if (mode === "local") {
      $("#proxy-local-port").value = p.port;
    }
    if (mode === "existing") {
      $("#proxy-exist-host").value = p.host || "127.0.0.1";
      $("#proxy-exist-port").value = p.port;
    }
  }

  const proto = [p.http ? "HTTP" : null, p.socks5 ? "SOCKS5" : null].filter(Boolean).join(" + ") || "—";
  const endpoint = p.endpoint || "未启用";
  const statusText = usable ? "可用" : p.enabled || p.running ? "不可用" : "未启用";

  const statusEl = $("#proxy-status");
  statusEl.textContent = statusText;
  statusEl.className = "status " + (usable ? "on" : "off");
  $("#proxy-endpoint-label").textContent = p.enabled || p.running ? endpoint : "未启用";
  $("#proxy-info-endpoint").textContent = p.endpoint || "—";
  $("#proxy-info-proto").textContent = proto;
  $("#proxy-info-checked").textContent = p.lastCheckedAt
    ? new Date(p.lastCheckedAt * 1000).toLocaleString()
    : "—";
  $("#proxy-info-message").textContent = p.message || "—";
  $("#proxy-side-endpoint").textContent = p.endpoint || "未启用";
  $("#proxy-side-proto").textContent = usable
    ? `${proto} · 其它面板可使用`
    : "其它面板可读取此状态";

  const ind = $("#proxy-indicator");
  const indText = $("#proxy-indicator-text");
  const indDot = $("#proxy-indicator-dot");
  ind.classList.toggle("on", usable);
  indDot.className = "status-dot " + (usable ? "on" : "off");
  indText.textContent = usable ? `代理 ${endpoint}` : "代理未启用";
  ind.title = p.message || "Current proxy (shared across panels)";

  $("#proxy-stop-btn").disabled = !(p.enabled || p.running);

  const live = currentProxy();
  const hint = live ? `代理 ${live.endpoint} · ${proto}` : "";
  ["#host-proxy-hint", "#tunnel-proxy-hint"].forEach((sel) => {
    const el = $(sel);
    if (!el) return;
    el.textContent = hint;
    el.classList.toggle("hidden", !usable);
  });
  updateInjectUI();
  updateUpdateBtnAlign();
}

// ---- rendering --------------------------------------------------------------

function makeEmptyItem(text) {
  const li = document.createElement("li");
  li.className = "list-empty";
  li.textContent = text;
  return li;
}

function makeItem({ selected, name, sub, active, onClick, actions }) {
  const li = document.createElement("li");
  li.className = "item" + (selected ? " selected" : "");

  const wrap = document.createElement("div");
  wrap.className = "item-wrap";

  const btn = document.createElement("button");
  btn.className = "item-btn";
  btn.type = "button";

  const info = document.createElement("div");
  info.className = "item-info";

  const nameEl = document.createElement("span");
  nameEl.className = "item-name";
  nameEl.textContent = name;

  const subEl = document.createElement("span");
  subEl.className = "item-sub";
  subEl.textContent = sub;

  info.append(nameEl, subEl);
  btn.appendChild(info);

  if (active !== undefined) {
    const dot = document.createElement("span");
    dot.className = "status-dot " + (active ? "on" : "off");
    btn.appendChild(dot);
  }

  btn.addEventListener("click", onClick);
  wrap.appendChild(btn);

  if (actions && actions.length) {
    const actionWrap = document.createElement("div");
    actionWrap.className = "item-actions";
    for (const action of actions) {
      const ab = document.createElement("button");
      ab.className = "item-action" + (action.danger ? " danger" : "");
      ab.type = "button";
      ab.title = action.title;
      ab.innerHTML = action.icon;
      ab.addEventListener("click", (e) => {
        e.stopPropagation();
        action.onClick();
      });
      actionWrap.appendChild(ab);
    }
    wrap.appendChild(actionWrap);
  }

  li.appendChild(wrap);
  return li;
}

function renderHostList() {
  hostListEl.textContent = "";
  if (state.hosts.length === 0) {
    hostListEl.appendChild(makeEmptyItem("暂无主机"));
    return;
  }
  for (const host of state.hosts) {
    hostListEl.appendChild(
      makeItem({
        selected: host.id === state.selectedHostId,
        name: host.name,
        sub: `${host.username}@${host.hostname}:${host.port}`,
        active: !!state.termId && host.id === state.connectedHostId,
        onClick: () => selectHost(host.id),
      })
    );
  }
}

function renderTunnelList() {
  tunnelListEl.textContent = "";
  if (state.tunnels.length === 0) {
    tunnelListEl.appendChild(makeEmptyItem("暂无本地隧道"));
    return;
  }
  for (const t of state.tunnels) {
    tunnelListEl.appendChild(
      makeItem({
        selected: t.id === state.selectedTunnelId,
        name: t.name,
        sub: `${t.localPort} → ${t.remoteHost}:${t.remotePort}`,
        active: t.active,
        onClick: () => selectTunnel(t.id),
      })
    );
  }
}

function renderCertList() {
  certListEl.textContent = "";
  if (state.certificates.length === 0) {
    certListEl.appendChild(makeEmptyItem("暂无本地证书"));
    return;
  }
  for (const cert of state.certificates) {
    certListEl.appendChild(
      makeItem({
        selected: cert.id === state.selectedCertId,
        name: cert.name,
        sub: "ed25519",
        onClick: () => selectCert(cert.id),
      })
    );
  }
}

function updateMainView() {
  hostViewEl.classList.add("hidden");
  tunnelViewEl.classList.add("hidden");
  certViewEl.classList.add("hidden");
  proxyViewEl.classList.add("hidden");
  emptyStateEl.classList.add("hidden");

  if (state.section === "hosts") {
    if (state.selectedHostId && hostById(state.selectedHostId)) {
      hostViewEl.classList.remove("hidden");
      return;
    }
    emptyTitleEl.textContent = "未选择主机";
    emptySubEl.textContent = "在左侧选择一台主机，或添加一台新主机。";
  } else if (state.section === "tunnels") {
    if (state.selectedTunnelId && tunnelById(state.selectedTunnelId)) {
      tunnelViewEl.classList.remove("hidden");
      return;
    }
    emptyTitleEl.textContent = "未选择本地隧道";
    emptySubEl.textContent = "在左侧选择一条隧道，或添加一条新隧道。";
  } else if (state.section === "certificates") {
    if (state.selectedCertId && certById(state.selectedCertId)) {
      certViewEl.classList.remove("hidden");
      return;
    }
    emptyTitleEl.textContent = "未选择本地证书";
    emptySubEl.textContent = "在左侧选择一张证书，或添加一张新证书。";
  } else {
    proxyViewEl.classList.remove("hidden");
    return;
  }
  emptyStateEl.classList.remove("hidden");
}

// ---- selection --------------------------------------------------------------

async function selectHost(id) {
  const host = hostById(id);
  if (!host) return;

  if (state.termId) {
    await disconnectTerminal();
  } else {
    term.reset();
    updateTerminalUI();
  }

  state.selectedHostId = id;
  hostNameEl.textContent = host.name;
  hostConnEl.textContent = `${host.username}@${host.hostname}:${host.port}`;
  renderHostList();
  updateMainView();
  updateInjectUI();
}

function selectTunnel(id) {
  const t = tunnelById(id);
  if (!t) return;
  state.selectedTunnelId = id;
  renderTunnelList();
  renderTunnelDetail();
  updateMainView();
}

function renderTunnelDetail() {
  const t = tunnelById(state.selectedTunnelId);
  if (!t) return;
  tunnelNameEl.textContent = t.name;
  tunnelStatusEl.textContent = t.active ? "Connected" : "Disconnected";
  tunnelStatusEl.className = "status " + (t.active ? "on" : "off");
  tLocalEl.textContent = `127.0.0.1:${t.localPort}`;
  tRemoteEl.textContent = `${t.remoteHost}:${t.remotePort}`;
  tSshEl.textContent = `${t.username}@${t.sshHost}:${t.sshPort}`;
  tAuthEl.textContent =
    t.auth.method === "certificate"
      ? `Certificate · ${certNameById(t.auth.certificateId)}`
      : "Password";
  toggleTunnelBtnEl.textContent = t.active ? "Disconnect" : "Connect";
  toggleTunnelBtnEl.className = "btn full " + (t.active ? "danger" : "primary");
}

function selectCert(id) {
  const cert = certById(id);
  if (!cert) return;
  state.selectedCertId = id;
  certNameEl.textContent = cert.name;
  cKeypathEl.textContent = cert.privateKeyPath;
  cPubkeyEl.textContent = cert.publicKey;
  renderCertList();
  updateMainView();
}

// ---- sections ---------------------------------------------------------------

function switchSection(section) {
  state.section = section;
  $("#nav-hosts").classList.toggle("active", section === "hosts");
  $("#nav-tunnels").classList.toggle("active", section === "tunnels");
  $("#nav-certificates").classList.toggle("active", section === "certificates");
  $("#nav-proxy").classList.toggle("active", section === "proxy");
  hostListEl.classList.toggle("hidden", section !== "hosts");
  tunnelListEl.classList.toggle("hidden", section !== "tunnels");
  certListEl.classList.toggle("hidden", section !== "certificates");
  proxySidebarEl.classList.toggle("hidden", section !== "proxy");
  addBtnEl.classList.toggle("hidden", section === "proxy");
  addBtnEl.textContent =
    section === "hosts"
      ? "+ 添加主机"
      : section === "tunnels"
        ? "+ 添加本地隧道"
        : "+ 添加本地证书";
  updateMainView();
}

// ---- certificate dropdown ---------------------------------------------------

function fillCertSelects() {
  [hostFormEl, tunnelFormEl].forEach((form) => {
    const select = form.elements.certificate_id;
    if (!select) return;
    const current = select.value;
    select.textContent = "";
    for (const cert of state.certificates) {
      const opt = document.createElement("option");
      opt.value = cert.id;
      opt.textContent = cert.name;
      select.appendChild(opt);
    }
    if (state.certificates.some((c) => c.id === current)) {
      select.value = current;
    }
  });
}

// ---- modals -----------------------------------------------------------------

function openHostModal(host) {
  state.editingHostId = host ? host.id : null;
  hostModalTitleEl.textContent = host ? "编辑主机" : "添加主机";
  fillCertSelects();

  const f = hostFormEl.elements;
  f.name.value = host ? host.name : "";
  f.hostname.value = host ? host.hostname : "";
  f.port.value = host ? host.port : 22;
  f.username.value = host ? host.username : "";
  f.inject_remote_port.value = host ? hostInjectRemotePort(host) : 7890;

  if (host && host.auth && host.auth.method === "certificate") {
    f.auth_method.value = "certificate";
    f.certificate_id.value = host.auth.certificateId;
    f.password.value = "";
  } else {
    f.auth_method.value = "password";
    f.certificate_id.value = "";
    f.password.value = host && host.auth ? host.auth.password : "";
  }

  updateHostAuthFields();
  hostModalEl.classList.remove("hidden");
  f.name.focus();
}

function closeHostModal() {
  hostModalEl.classList.add("hidden");
}

function openTunnelModal(tunnel) {
  state.editingTunnelId = tunnel ? tunnel.id : null;
  tunnelModalTitleEl.textContent = tunnel ? "编辑本地隧道" : "添加本地隧道";
  fillCertSelects();

  const f = tunnelFormEl.elements;
  f.name.value = tunnel ? tunnel.name : "";
  f.local_port.value = tunnel ? tunnel.localPort : "";
  f.remote_host.value = tunnel ? tunnel.remoteHost : "";
  f.remote_port.value = tunnel ? tunnel.remotePort : "";
  f.ssh_host.value = tunnel ? tunnel.sshHost : "";
  f.ssh_port.value = tunnel ? tunnel.sshPort : 22;
  f.username.value = tunnel ? tunnel.username : "";
  sshCmdInputEl.value = "";

  if (tunnel && tunnel.auth && tunnel.auth.method === "certificate") {
    f.auth_method.value = "certificate";
    f.certificate_id.value = tunnel.auth.certificateId;
    f.password.value = "";
  } else {
    f.auth_method.value = "password";
    f.certificate_id.value = "";
    f.password.value = tunnel && tunnel.auth ? tunnel.auth.password : "";
  }

  updateTunnelAuthFields();
  tunnelModalEl.classList.remove("hidden");
  f.name.focus();
}

function closeTunnelModal() {
  tunnelModalEl.classList.add("hidden");
}

function openCertModal() {
  certFormEl.elements.name.value = "";
  certModalEl.classList.remove("hidden");
  certFormEl.elements.name.focus();
}

function closeCertModal() {
  certModalEl.classList.add("hidden");
}

function updateHostAuthFields() {
  const method = hostFormEl.elements.auth_method.value;
  hostAuthPasswordEl.classList.toggle("hidden", method !== "password");
  hostAuthCertEl.classList.toggle("hidden", method !== "certificate");
}

function updateTunnelAuthFields() {
  const method = tunnelFormEl.elements.auth_method.value;
  tunnelAuthPasswordEl.classList.toggle("hidden", method !== "password");
  tunnelAuthCertEl.classList.toggle("hidden", method !== "certificate");
}

// ---- actions ----------------------------------------------------------------

async function deleteHost(host) {
  if (!confirm(`Delete host "${host.name}"?`)) return;
  try {
    if (state.selectedHostId === host.id && state.termId) {
      await disconnectTerminal();
    }
    await invoke("delete_host", { id: host.id });
    delete state.injections[host.id];
    if (state.selectedHostId === host.id) state.selectedHostId = null;
    await loadHosts();
    updateMainView();
  } catch (err) {
    alert(`Error: ${err}`);
  }
}

async function deleteSelectedTunnel() {
  const t = tunnelById(state.selectedTunnelId);
  if (!t) return;
  if (!confirm(`Delete tunnel "${t.name}"?`)) return;
  try {
    await invoke("delete_tunnel", { id: t.id });
    state.selectedTunnelId = null;
    await loadTunnels();
    updateMainView();
  } catch (err) {
    alert(`Error: ${err}`);
  }
}

async function deleteSelectedCert() {
  const cert = certById(state.selectedCertId);
  if (!cert) return;
  if (!confirm(`Delete certificate "${cert.name}"?`)) return;
  try {
    await invoke("delete_certificate", { id: cert.id });
    state.selectedCertId = null;
    await loadCertificates();
    updateMainView();
  } catch (err) {
    alert(`Error: ${err}`);
  }
}

async function toggleTunnel() {
  const t = tunnelById(state.selectedTunnelId);
  if (!t) return;
  toggleTunnelBtnEl.disabled = true;
  toggleTunnelBtnEl.textContent = t.active ? "Disconnecting…" : "Connecting…";
  try {
    if (t.active) {
      await invoke("tunnel_disconnect", { tunnelId: t.id });
    } else {
      await invoke("tunnel_connect", { tunnelId: t.id });
    }
    await loadTunnels();
    renderTunnelDetail();
  } catch (err) {
    alert(`Error: ${err}`);
    await loadTunnels();
    renderTunnelDetail();
  } finally {
    toggleTunnelBtnEl.disabled = false;
  }
}

async function parseSshCommand() {
  const cmd = sshCmdInputEl.value.trim();
  if (!cmd) return;
  try {
    const t = await invoke("parse_ssh_command", { command: cmd });
    const f = tunnelFormEl.elements;
    f.local_port.value = t.localPort;
    f.remote_host.value = t.remoteHost;
    f.remote_port.value = t.remotePort;
    f.ssh_host.value = t.sshHost;
    f.ssh_port.value = t.sshPort;
    f.username.value = t.username;
  } catch (err) {
    alert(`Parse failed: ${err}`);
  }
}

// ---- events -----------------------------------------------------------------

$("#nav-hosts").addEventListener("click", () => switchSection("hosts"));
$("#nav-tunnels").addEventListener("click", () => switchSection("tunnels"));
$("#nav-certificates").addEventListener("click", () => switchSection("certificates"));
$("#nav-proxy").addEventListener("click", () => switchSection("proxy"));
$("#proxy-indicator").addEventListener("click", () => switchSection("proxy"));

addBtnEl.addEventListener("click", () => {
  if (state.section === "hosts") openHostModal(null);
  else if (state.section === "tunnels") openTunnelModal(null);
  else if (state.section === "certificates") openCertModal();
});

$("#cancel-host-btn").addEventListener("click", closeHostModal);
hostModalEl.addEventListener("click", (e) => {
  if (e.target === hostModalEl) closeHostModal();
});

$("#edit-tunnel-btn").addEventListener("click", () => {
  const t = tunnelById(state.selectedTunnelId);
  if (t) openTunnelModal(t);
});
$("#delete-tunnel-btn").addEventListener("click", deleteSelectedTunnel);
$("#cancel-tunnel-btn").addEventListener("click", closeTunnelModal);
$("#parse-btn").addEventListener("click", parseSshCommand);
tunnelModalEl.addEventListener("click", (e) => {
  if (e.target === tunnelModalEl) closeTunnelModal();
});

$("#delete-cert-btn").addEventListener("click", deleteSelectedCert);
$("#cancel-cert-btn").addEventListener("click", closeCertModal);
certModalEl.addEventListener("click", (e) => {
  if (e.target === certModalEl) closeCertModal();
});

toggleTunnelBtnEl.addEventListener("click", toggleTunnel);
termToggleBtnEl.addEventListener("click", toggleTerminal);
injectBtnEl.addEventListener("click", toggleInject);
$("#edit-host-btn").addEventListener("click", () => {
  const host = hostById(state.selectedHostId);
  if (host) openHostModal(host);
});
$("#delete-host-btn").addEventListener("click", () => {
  const host = hostById(state.selectedHostId);
  if (host) deleteHost(host);
});
updateBtnEl.addEventListener("click", onCanglingUpdateClick);
appUpdateBtnEl.addEventListener("click", onAppUpdateClick);

hostFormEl.querySelectorAll('input[name="auth_method"]').forEach((r) => {
  r.addEventListener("change", updateHostAuthFields);
});
tunnelFormEl.querySelectorAll('input[name="auth_method"]').forEach((r) => {
  r.addEventListener("change", updateTunnelAuthFields);
});

hostFormEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = hostFormEl.elements;
  const method = f.auth_method.value;
  const host = {
    name: f.name.value.trim(),
    hostname: f.hostname.value.trim(),
    port: parseInt(f.port.value, 10) || 22,
    username: f.username.value.trim(),
    inject_remote_port: parseInt(f.inject_remote_port.value, 10) || 7890,
    auth:
      method === "certificate"
        ? { method: "certificate", certificateId: f.certificate_id.value }
        : { method: "password", password: f.password.value },
  };
  try {
    const editingId = state.editingHostId;
    if (editingId) {
      host.id = editingId;
      await invoke("update_host", { host });
    } else {
      const added = await invoke("add_host", { host });
      state.selectedHostId = added.id;
    }
    closeHostModal();
    await loadHosts();
    selectHost(editingId || state.selectedHostId);
  } catch (err) {
    alert(`Error: ${err}`);
  }
});

tunnelFormEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = tunnelFormEl.elements;
  const method = f.auth_method.value;
  const tunnel = {
    name: f.name.value.trim(),
    localPort: parseInt(f.local_port.value, 10) || 0,
    remoteHost: f.remote_host.value.trim(),
    remotePort: parseInt(f.remote_port.value, 10) || 0,
    sshHost: f.ssh_host.value.trim(),
    sshPort: parseInt(f.ssh_port.value, 10) || 22,
    username: f.username.value.trim(),
    auth:
      method === "certificate"
        ? { method: "certificate", certificateId: f.certificate_id.value }
        : { method: "password", password: f.password.value },
  };
  try {
    const editingId = state.editingTunnelId;
    if (editingId) {
      tunnel.id = editingId;
      await invoke("update_tunnel", { tunnel });
    } else {
      const added = await invoke("add_tunnel", { tunnel });
      state.selectedTunnelId = added.id;
    }
    closeTunnelModal();
    await loadTunnels();
    selectTunnel(editingId || state.selectedTunnelId);
  } catch (err) {
    alert(`Error: ${err}`);
  }
});

certFormEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const name = certFormEl.elements.name.value.trim();
  if (!name) return;
  try {
    const cert = await invoke("add_certificate", { name });
    closeCertModal();
    await loadCertificates();
    selectCert(cert.id);
  } catch (err) {
    alert(`Error: ${err}`);
  }
});

listen("tunnel-stopped", async () => {
  await loadTunnels();
  renderTunnelDetail();
});

listen("proxy-status", (e) => {
  applyProxyStatus(e.payload);
});

listen("proxy-injection", (e) => {
  const info = e.payload;
  if (!info || !info.hostId) return;
  state.injections[info.hostId] = info;
  if (info.hostId === state.selectedHostId) {
    updateInjectUI();
    if (state.termId) probeCanglingUpdate();
  }
});

function selectedProxyMode() {
  const el = document.querySelector('input[name="proxy_mode"]:checked');
  return el ? el.value : "local";
}

document.querySelectorAll('input[name="proxy_mode"]').forEach((r) => {
  r.addEventListener("change", () => {
    const mode = selectedProxyMode();
    $("#proxy-option-local").classList.toggle("selected", mode === "local");
    $("#proxy-option-existing").classList.toggle("selected", mode === "existing");
  });
});

$("#proxy-option-local").addEventListener("click", (e) => {
  if (e.target.closest("button, input[type='number'], input[type='text']")) return;
  document.querySelector('input[name="proxy_mode"][value="local"]').checked = true;
  document.querySelector('input[name="proxy_mode"][value="local"]').dispatchEvent(new Event("change"));
});
$("#proxy-option-existing").addEventListener("click", (e) => {
  if (e.target.closest("button, input[type='number'], input[type='text']")) return;
  document.querySelector('input[name="proxy_mode"][value="existing"]').checked = true;
  document.querySelector('input[name="proxy_mode"][value="existing"]').dispatchEvent(new Event("change"));
});

async function startLocalProxy() {
  const port = parseInt($("#proxy-local-port").value, 10) || 0;
  const btn = $("#proxy-start-btn");
  btn.disabled = true;
  btn.textContent = "启动中…";
  try {
    const status = await invoke("start_local_proxy", { port });
    applyProxyStatus(status);
    if (!proxyIsUsable(status)) {
      alert(status.message || "Proxy started but probe failed");
    }
  } catch (err) {
    alert(`Error: ${err}`);
  } finally {
    btn.disabled = false;
    btn.textContent = "启动";
  }
}

async function useExistingProxy() {
  const host = $("#proxy-exist-host").value.trim();
  const port = parseInt($("#proxy-exist-port").value, 10) || 0;
  const btn = $("#proxy-use-btn");
  btn.disabled = true;
  btn.textContent = "检测中…";
  try {
    const status = await invoke("use_existing_proxy", { host, port });
    applyProxyStatus(status);
    if (!proxyIsUsable(status)) {
      alert(status.message || "Existing proxy is not usable");
    }
  } catch (err) {
    alert(`Error: ${err}`);
  } finally {
    btn.disabled = false;
    btn.textContent = "使用";
  }
}

async function checkProxy() {
  const btn = $("#proxy-check-btn");
  btn.disabled = true;
  try {
    const status = await invoke("check_proxy");
    applyProxyStatus(status);
  } catch (err) {
    alert(`Error: ${err}`);
  } finally {
    btn.disabled = false;
  }
}

async function stopProxy() {
  try {
    const status = await invoke("stop_proxy");
    applyProxyStatus(status);
  } catch (err) {
    alert(`Error: ${err}`);
  }
}

$("#proxy-start-btn").addEventListener("click", startLocalProxy);
$("#proxy-use-btn").addEventListener("click", useExistingProxy);
$("#proxy-check-btn").addEventListener("click", checkProxy);
$("#proxy-stop-btn").addEventListener("click", stopProxy);

listen("terminal-data", (e) => {
  if (e.payload.id === state.termId && term) {
    term.write(e.payload.data);
  }
});

listen("terminal-closed", (e) => {
  if (e.payload === state.termId) {
    state.termId = null;
    state.connectedHostId = null;
    updateTerminalUI();
    if (term) term.write("\r\n\x1b[90m[disconnected]\x1b[0m\r\n");
  }
});

// Double-click an empty input to fill it with its placeholder text.
document.addEventListener("dblclick", (event) => {
  const el = event.target;
  if (
    el instanceof HTMLInputElement &&
    el.type !== "password" &&
    el.placeholder &&
    el.value.trim() === ""
  ) {
    el.value = el.placeholder;
  }
});

// ---- init -------------------------------------------------------------------

(async () => {
  initTerminal();
  updateTerminalUI();
  checkAppUpdate();
  try {
    await Promise.all([
      loadHosts(),
      loadTunnels(),
      loadCertificates(),
      loadProxy(),
      loadInjections(),
    ]);
    const p = state.proxy;
    if (p && p.mode === "local" && p.enabled && p.port && !p.running) {
      try {
        applyProxyStatus(await invoke("start_local_proxy", { port: p.port }));
      } catch (_) {
        await loadProxy();
      }
    } else if (p && p.mode === "existing" && p.port) {
      try {
        applyProxyStatus(await invoke("check_proxy"));
      } catch (_) {}
    }
  } catch (err) {
    alert(`Failed to load data: ${err}`);
  }
})();
