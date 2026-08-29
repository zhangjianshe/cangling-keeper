// `withGlobalTauri: true` in tauri.conf.json injects the API at window.__TAURI__.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  section: "hosts", // "hosts" | "tunnels" | "certificates" | "proxy" | "repo"
  hosts: [],
  searchHosts: "",
  collapsedGroups: {}, // host catalog group name -> true when collapsed
  tunnels: [], // [{...tunnel, active}]
  certificates: [],
  selectedHostId: null,
  selectedTunnelId: null,
  selectedCertId: null,
  editingHostId: null,
  editingTunnelId: null,
  contextHostId: null,
  contextSetName: null,
  termId: null,
  connectedHostId: null,
  proxy: null, // shared ProxyStatus for every panel
  injections: {}, // hostId -> { active, remotePort, localEndpoint }
  updateProbe: null,
  updateBusy: false,
  roleBusy: false,
  appUpdate: null,
  appUpdateBusy: false,
  login: null, // { loggedIn, serverUrl, username, nickname }
  repoStatus: null, // { cloned, localPath, setName, totalFiles, downloaded, skipped, failed, error }
  softwareSets: [], // [{ name, kind, cloned, localPath, gitUrl, ... }]
  editingSetName: null,
  selectedSetName: "",
  repoPath: "", // relative path inside the synced set ("" = root)
  repoSyncing: false,
  hostSyncing: false,
  clusterPorts: {}, // hostId -> cangling-update listen port
  clusterFrameUrl: "",
  clusterForwardHostId: "",
  clusterLocalPort: 0,
  clusterConnecting: false,
  clusterConnectGen: 0,
};

const $ = (sel) => document.querySelector(sel);

const addBtnEl = $("#add-btn");
const sidebarAddRowEl = $("#sidebar-add-row");
const hostSearchBoxEl = $("#host-search-box");
const hostSearchInputEl = $("#host-search-input");
const hostListEl = $("#host-list");
const tunnelListEl = $("#tunnel-list");
const certListEl = $("#cert-list");
const setListEl = $("#set-list");
const emptyStateEl = $("#empty-state");
const emptyTitleEl = $("#empty-title");
const emptySubEl = $("#empty-sub");
const hostViewEl = $("#host-view");
const tunnelViewEl = $("#tunnel-view");
const certViewEl = $("#cert-view");
const proxyViewEl = $("#proxy-view");
const repoViewEl = $("#repo-view");
const proxySidebarEl = $("#proxy-sidebar");

const hostNameEl = $("#host-name");
const hostConnEl = $("#host-conn");
const termToggleBtnEl = $("#term-toggle-btn");
const termStatusEl = $("#term-status");
const checkEnvBtnEl = $("#check-env-btn");
const resourceMgrBtnEl = $("#resource-mgr-btn");
const softwareSyncBtnEl = $("#software-sync-btn");
const terminalFrameEl = $("#terminal-frame");
const clusterFrameEl = $("#cluster-frame");
const clusterFrameUrlEl = $("#cluster-frame-url");
const clusterIframeEl = $("#cluster-iframe");
const clusterFrameRefreshEl = $("#cluster-frame-refresh");
const clusterFrameExternalEl = $("#cluster-frame-external");
const clusterFrameCloseEl = $("#cluster-frame-close");
const hostSyncProgressEl = $("#host-sync-progress");
const hostSyncProgressLabelEl = $("#host-sync-progress-label");
const hostSyncProgressPctEl = $("#host-sync-progress-pct");
const hostSyncProgressBarEl = $("#host-sync-progress-bar");
const hostSyncProgressDetailEl = $("#host-sync-progress-detail");
const injectBtnEl = $("#inject-proxy-btn");
const injectStatusEl = $("#inject-status");
const updateBtnEl = $("#btn-cangling-update");
const roleSwitchEl = $("#role-switch");
const updateRowEl = updateBtnEl ? updateBtnEl.closest(".header-update-row") : null;
const clusterTokenRowEl = $("#cluster-token-row");
const clusterTokenValueEl = $("#cluster-token-value");
const clusterTokenCopyEl = $("#cluster-token-copy");
const appUpdateBtnEl = $("#app-update-btn");
const loginBtnEl = $("#login-btn");
const loginModalEl = $("#login-modal");
const loginFormEl = $("#login-form");
const loginStatusEl = $("#login-status");
const syncBtnEl = $("#sync-btn");
const logoutBtnEl = $("#logout-btn");
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

const repoStatusLineEl = $("#repo-status-line");
const repoSetTitleEl = $("#repo-set-title");
const repoPathEl = $("#repo-path");
const repoCloneUpdateBtnEl = $("#repo-clone-update-btn");
const setModalEl = $("#set-modal");
const setFormEl = $("#set-form");
const setModalTitleEl = $("#set-modal-title");
const repoProgressEl = $("#repo-progress");
const repoProgressLabelEl = $("#repo-progress-label");
const repoProgressPctEl = $("#repo-progress-pct");
const repoProgressBarEl = $("#repo-progress-bar");
const repoProgressDetailEl = $("#repo-progress-detail");
const repoUpBtnEl = $("#repo-up-btn");
const repoCurrentPathEl = $("#repo-current-path");
const repoTreeEl = $("#repo-tree");
const repoFileTitleEl = $("#repo-file-title");
const repoFileContentEl = $("#repo-file-content");

const ctxMenuEl = $("#ctx-menu");
const ctxEditHostBtnEl = $("#ctx-edit-host");
const ctxCopyHostBtnEl = $("#ctx-copy-host");
const ctxDeleteHostBtnEl = $("#ctx-delete-host");
const setCtxMenuEl = $("#set-ctx-menu");
const ctxEditSetBtnEl = $("#ctx-edit-set");
const ctxDeleteSetBtnEl = $("#ctx-delete-set");

// ---- xterm.js terminal ------------------------------------------------------

let term = null;
let fitAddon = null;

async function ensureTerminalFont() {
  try {
    await Promise.all([
      document.fonts.load('16px "Iosevka Term Web"'),
      document.fonts.load('italic 16px "Iosevka Term Web"'),
      document.fonts.load('700 16px "Iosevka Term Web"'),
      document.fonts.load('italic 700 16px "Iosevka Term Web"'),
    ]);
  } catch (_) {
    // Fall back to the CSS monospace stack if the bundled font fails to load.
  }
}

function initTerminal() {
  term = new Terminal({
    cursorBlink: true,
    fontSize: 16,
    fontFamily: '"Iosevka Term Web", "Cascadia Mono", "SF Mono", Menlo, Consolas, monospace',
    scrollback: 10000,
    theme: {
      background: "#ffffff",
      foreground: "#1f2328",
      cursor: "#0969da",
      cursorAccent: "#ffffff",
      selectionBackground: "#ddf4ff",
      black: "#24292e",
      red: "#cf222e",
      green: "#116329",
      yellow: "#4d2d00",
      blue: "#0969da",
      magenta: "#8250df",
      cyan: "#1b7c83",
      white: "#6e7781",
      brightBlack: "#57606a",
      brightRed: "#a40e26",
      brightGreen: "#1a7f37",
      brightYellow: "#633c01",
      brightBlue: "#218bff",
      brightMagenta: "#a475f9",
      brightCyan: "#3192aa",
      brightWhite: "#8c959f",
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

  hideClusterFrame();
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
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Error: ${err}`);
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
  const left = !currentProxy();
  updateBtnEl.classList.toggle("align-left", left);
  if (updateRowEl) updateRowEl.classList.toggle("align-left", left);
}

function currentRole() {
  const p = state.updateProbe;
  const role = p && p.role ? String(p.role).toLowerCase() : "";
  if (role === "master" || role === "worker" || role === "standalone") return role;
  return p && p.installed ? "standalone" : "";
}

function roleLabel(role) {
  if (role === "master") return "Master";
  if (role === "worker") return "Worker";
  return "独立模式";
}

function clusterToken() {
  const p = state.updateProbe;
  return (p && (p.clusterToken || p.token) ? String(p.clusterToken || p.token) : "").trim();
}

function renderClusterToken() {
  const show = !!state.termId && currentRole() === "master";
  const token = clusterToken();
  clusterTokenRowEl.classList.toggle("hidden", !show);
  clusterTokenValueEl.textContent = show ? token || "未设置" : "";
  clusterTokenValueEl.title = token;
  clusterTokenCopyEl.disabled = !token;
}

async function copyClusterToken() {
  const token = clusterToken();
  if (!token) return;
  let ok = false;
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(token);
      ok = true;
    }
  } catch (_) {}
  if (!ok) {
    const ta = document.createElement("textarea");
    ta.value = token;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    ok = document.execCommand("copy");
    ta.remove();
  }
  const prev = clusterTokenCopyEl.textContent;
  clusterTokenCopyEl.textContent = ok ? "已复制" : "复制失败";
  setTimeout(() => {
    clusterTokenCopyEl.textContent = prev;
  }, 1500);
  if (!ok) uiAlert("复制 cluster token 失败");
}

function renderRoleSwitch() {
  const connected = !!state.termId;
  const p = state.updateProbe;
  const installed = !!(p && p.installed);
  const busy = state.roleBusy || state.updateBusy;
  const role = currentRole();
  renderClusterToken();

  if (!connected || !installed) {
    roleSwitchEl.classList.add("hidden");
    roleSwitchEl.classList.remove("disabled");
    roleSwitchEl.querySelectorAll(".seg-opt").forEach((btn) => {
      btn.classList.remove("active");
      btn.setAttribute("aria-checked", "false");
      btn.disabled = true;
    });
    return;
  }

  roleSwitchEl.classList.remove("hidden");
  roleSwitchEl.classList.toggle("disabled", busy);
  roleSwitchEl.querySelectorAll(".seg-opt").forEach((btn) => {
    const on = btn.dataset.role === role;
    btn.classList.toggle("active", on);
    btn.setAttribute("aria-checked", on ? "true" : "false");
    btn.disabled = busy;
  });
  roleSwitchEl.title = busy
    ? "正在切换运行模式…"
    : `当前运行模式：${roleLabel(role)}${p && p.master ? ` · master ${p.master}` : ""}`;
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

function renderSoftwareSyncBtn() {
  if (!softwareSyncBtnEl) return;
  const hasHost = !!hostById(state.selectedHostId);
  softwareSyncBtnEl.disabled = !hasHost || state.hostSyncing;
  softwareSyncBtnEl.textContent = state.hostSyncing ? "同步中…" : "软件同步";
}

function hideHostSyncProgress() {
  if (!hostSyncProgressEl) return;
  hostSyncProgressEl.classList.add("hidden");
  hostSyncProgressBarEl.style.width = "0%";
  hostSyncProgressPctEl.textContent = "0%";
  hostSyncProgressLabelEl.textContent = "同步中";
  hostSyncProgressDetailEl.textContent = "";
}

function renderHostSyncProgress(p) {
  if (!hostSyncProgressEl) return;
  hostSyncProgressEl.classList.remove("hidden");
  const current = p.current || 0;
  const total = p.total || 0;
  const overallDone = Number(p.overallDone) || 0;
  const overallTotal = Number(p.overallTotal) || 0;
  const bytesDone = Number(p.bytesDone) || 0;
  const bytesTotal = Number(p.bytesTotal) || 0;
  let pct = 0;
  if (overallTotal > 0) {
    pct = Math.min(100, (overallDone / overallTotal) * 100);
  } else if (total > 0) {
    pct = Math.min(100, (current / total) * 100);
  }
  const action =
    p.action === "upload"
      ? "上传"
      : p.action === "skip"
        ? "跳过"
        : p.action === "fail"
          ? "失败"
          : p.action || "同步";
  hostSyncProgressLabelEl.textContent =
    total > 0 ? `软件同步 ${current}/${total} · ${action}` : `软件同步 · ${action}`;
  hostSyncProgressPctEl.textContent = `${pct.toFixed(pct >= 10 ? 0 : 1)}%`;
  hostSyncProgressBarEl.style.width = `${pct}%`;
  const sizePart =
    p.action === "upload" && (bytesDone > 0 || bytesTotal > 0)
      ? bytesTotal
        ? `${formatBytes(bytesDone)} / ${formatBytes(bytesTotal)}`
        : formatBytes(bytesDone)
      : "";
  hostSyncProgressDetailEl.textContent = [p.file || "", sizePart, p.remotePath || ""]
    .filter(Boolean)
    .join(" · ");
}

function hostHttpHost(hostname) {
  const h = String(hostname || "").trim();
  if (!h) return "";
  if (h.includes(":") && !h.startsWith("[")) return `[${h}]`;
  return h;
}

function clusterMgrPort(hostId) {
  const p = state.updateProbe;
  if (hostId && state.selectedHostId === hostId && p && Number(p.port) > 0) {
    return Number(p.port);
  }
  const cached = hostId ? Number(state.clusterPorts[hostId] || 0) : 0;
  return cached > 0 ? cached : 0;
}

function clusterMgrUrl(host) {
  if (!host || !host.hostname) return "";
  const hostPart = hostHttpHost(host.hostname);
  const port = clusterMgrPort(host.id);
  const base = port > 0 ? `http://${hostPart}:${port}` : `http://${hostPart}`;
  return `${base}/console`;
}

function renderClusterMgrBtn() {
  if (!resourceMgrBtnEl) return;
  const host = hostById(state.selectedHostId);
  const url = state.clusterFrameUrl || clusterMgrUrl(host);
  const open = clusterFrameEl && !clusterFrameEl.classList.contains("hidden");
  resourceMgrBtnEl.disabled = !host || state.clusterConnecting;
  resourceMgrBtnEl.classList.toggle("active", !!open);
  if (state.clusterConnecting) {
    resourceMgrBtnEl.title = "正在通过 SSH 连接 cangling-update 控制台…";
  } else if (open && url) {
    resourceMgrBtnEl.title = `已连接 ${url}`;
  } else {
    resourceMgrBtnEl.title = "通过 SSH 打开该主机 cangling-update 控制台";
  }
}

function hideClusterFrame() {
  state.clusterConnectGen += 1;
  const hostId = state.clusterForwardHostId;
  state.clusterForwardHostId = "";
  state.clusterLocalPort = 0;
  state.clusterConnecting = false;
  if (!clusterFrameEl) {
    if (hostId) invoke("disconnect_update_console", { hostId }).catch(() => {});
    return;
  }
  clusterFrameEl.classList.add("hidden");
  if (terminalFrameEl) terminalFrameEl.classList.remove("hidden");
  if (clusterIframeEl) clusterIframeEl.src = "about:blank";
  state.clusterFrameUrl = "";
  if (clusterFrameUrlEl) clusterFrameUrlEl.textContent = "";
  if (hostId) invoke("disconnect_update_console", { hostId }).catch(() => {});
  if (fitAddon && terminalEl && terminalEl.offsetWidth > 0 && terminalEl.offsetHeight > 0) {
    fitAddon.fit();
  }
  renderClusterMgrBtn();
}

function showClusterFrame(url, remotePort) {
  if (!clusterFrameEl || !clusterIframeEl || !url) return;
  state.clusterFrameUrl = url;
  if (clusterFrameUrlEl) {
    const rp = Number(remotePort) || 0;
    const label =
      rp > 0 && !url.includes(`:${rp}/`) && !url.endsWith(`:${rp}`)
        ? `${url}  （远端 :${rp}）`
        : url;
    clusterFrameUrlEl.textContent = label;
    clusterFrameUrlEl.title = label;
  }
  if (terminalFrameEl) terminalFrameEl.classList.add("hidden");
  clusterFrameEl.classList.remove("hidden");
  if (clusterIframeEl.src !== url) clusterIframeEl.src = url;
  renderClusterMgrBtn();
}

function updateHostActionsUI() {
  renderSoftwareSyncBtn();
  renderClusterMgrBtn();
  const connected = !!state.termId;
  if (!connected) {
    setUpdateButton({ disabled: true, hidden: true });
    renderRoleSwitch();
    return;
  }

  renderRoleSwitch();

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
  if (state.updateBusy || state.roleBusy) return;

  const injected = hostInjected(hostId);
  if (injected) {
    setUpdateButton({ disabled: true, text: "检测中…", title: "正在检查 cangling-update…" });
  }
  try {
    state.updateProbe = await invoke("probe_cangling_update", { hostId });
  } catch (err) {
    state.updateProbe = null;
    renderRoleSwitch();
    if (injected) {
      setUpdateButton({ disabled: false, text: "重新检测", title: String(err), cls: "error" });
    } else {
      updateHostActionsUI();
    }
    return;
  }
  const port = Number(state.updateProbe && state.updateProbe.port) || 0;
  if (port > 0) state.clusterPorts[hostId] = port;
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
  renderRoleSwitch();
  try {
    const result = await invoke("run_cangling_update", { hostId });
    writeActionLog(
      result.action === "install" ? "安装更新程序" : "更新程序",
      [result.stdout, result.stderr].filter(Boolean).join("\n")
    );
  } catch (err) {
    writeActionLog("更新程序失败", String(err));
    uiAlert(`Error: ${err}`);
  } finally {
    state.updateBusy = false;
    await probeCanglingUpdate();
  }
}

async function onRoleSwitchClick(role) {
  const hostId = state.selectedHostId;
  if (!hostId || !state.termId) return;
  if (state.roleBusy || state.updateBusy) return;
  const p = state.updateProbe;
  if (!p || !p.installed) {
    uiAlert("请先安装更新程序");
    return;
  }

  const current = currentRole();
  let token = "";
  let master = p.master || "";

  if (role === "master" || role === "worker") {
    if (!p.tokenSet) {
      const entered = await uiPrompt(
        "集群共享令牌（master / worker 必须一致）",
        "",
        "集群令牌"
      );
      if (entered == null) return;
      token = String(entered).trim();
      if (!token) {
        uiAlert("集群角色需要填写共享令牌");
        return;
      }
    }
    if (role === "worker") {
      const entered = await uiPrompt(
        "主节点地址，例如 http://10.0.0.1:80（留空则 UDP 自动发现）",
        master,
        "主节点"
      );
      if (entered == null) return;
      master = String(entered).trim();
    } else {
      master = "";
    }
  }

  const extra =
    role === "worker" && master
      ? `，主节点 ${master}`
      : role === "worker"
        ? "，自动发现主节点"
        : "";
  const same = current === role ? "重新" : "";
  if (
    !(await uiConfirm(
      `将把本机 cangling-update 服务${same}注册为「${roleLabel(role)}」并重启${extra}。继续？`,
      "切换运行模式"
    ))
  ) {
    return;
  }

  state.roleBusy = true;
  renderRoleSwitch();
  try {
    const result = await invoke("set_cangling_role", {
      hostId,
      role,
      token,
      master,
    });
    writeActionLog(
      `运行模式 → ${roleLabel(result.role)}`,
      [result.stdout, result.stderr].filter(Boolean).join("\n")
    );
    if (state.updateProbe) {
      state.updateProbe.role = result.role;
      state.updateProbe.active = result.active;
      state.updateProbe.tokenSet = result.tokenSet;
      state.updateProbe.master = result.master || "";
      if (token) state.updateProbe.clusterToken = token;
    }
  } catch (err) {
    writeActionLog("切换运行模式失败", String(err));
    uiAlert(`切换运行模式失败: ${err}`);
  } finally {
    state.roleBusy = false;
    await probeCanglingUpdate();
  }
}

async function onCanglingUpdateClick() {
  const hostId = state.selectedHostId;
  if (!hostId || !state.termId) return;
  if (state.updateBusy) return;
  if (!hostInjected(hostId)) {
    uiAlert("请先注入代理");
    return;
  }

  // Refresh the probe so the install/update decision uses the current version.
  await probeCanglingUpdate();
  const p = state.updateProbe;
  if (!p) return;

  if (!p.supported) {
    uiAlert(`不支持的架构：${p.arch}`);
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
    uiAlert(`获取最新版本失败：${p.versionError}`);
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
  if (!(await uiConfirm(`发现新版本 ${s.latest}（当前 ${s.current}），现在下载并更新？`))) {
    return;
  }
  state.appUpdateBusy = true;
  appUpdateBtnEl.disabled = true;
  appUpdateBtnEl.textContent = "下载更新中…";
  try {
    await invoke("apply_app_update");
  } catch (err) {
    state.appUpdateBusy = false;
    uiAlert(`更新失败: ${err}`);
    renderAppUpdate();
  }
}

// ---- login & host sync -----------------------------------------------------

function renderLoginStatus() {
  const s = state.login || { loggedIn: false };
  if (s.loggedIn) {
    loginBtnEl.textContent = s.nickname || s.username || "已登录";
    loginBtnEl.title = `已登录 ${s.username} · ${s.serverUrl}`;
    loginBtnEl.classList.add("logged-in");
  } else {
    loginBtnEl.textContent = "登录";
    loginBtnEl.title = "登录到维护中心服务器";
    loginBtnEl.classList.remove("logged-in");
  }
  updateSyncBtn();
}

function updateSyncBtn() {
  const s = state.login || { loggedIn: false };
  syncBtnEl.classList.toggle("hidden", state.section !== "hosts" || !s.loggedIn);
}

function openLoginModal() {
  const s = state.login || {};
  const f = loginFormEl.elements;
  f.server_url.value = s.serverUrl || "https://soft.cangling.cn:22002";
  f.username.value = s.username || "";
  f.password.value = "";
  loginStatusEl.textContent = "";
  loginStatusEl.classList.add("hidden");
  logoutBtnEl.classList.toggle("hidden", !s.loggedIn);
  loginModalEl.classList.remove("hidden");
  (s.loggedIn ? logoutBtnEl : f.username).focus();
}

function closeLoginModal() {
  loginModalEl.classList.add("hidden");
}

async function loadLoginStatus() {
  try {
    state.login = await invoke("get_login_status");
  } catch (_) {
    state.login = { loggedIn: false, serverUrl: "", username: "", nickname: "" };
  }
  renderLoginStatus();
}

async function autoSyncIfLoggedIn() {
  const s = state.login || {};
  if (!s.loggedIn) return;
  try {
    await invoke("sync_public_hosts");
    await loadHosts();
    await loadCertificates();
    updateMainView();
  } catch (_) {
    // Background sync is best-effort; the user can still sync manually.
  }
}

async function onSyncClick() {
  const s = state.login || {};
  if (!s.loggedIn) {
    openLoginModal();
    return;
  }
  syncBtnEl.disabled = true;
  syncBtnEl.textContent = "同步中…";
  syncBtnEl.title = "正在同步主机…";
  try {
    await invoke("sync_hosts");
    await loadHosts();
    await loadCertificates();
    updateMainView();
    syncBtnEl.textContent = "已同步";
    syncBtnEl.title = `已同步 · ${new Date().toLocaleTimeString()}`;
  } catch (err) {
    syncBtnEl.textContent = "同步失败";
    syncBtnEl.title = String(err);
    uiAlert(`同步失败: ${err}`);
  } finally {
    syncBtnEl.disabled = false;
    setTimeout(() => {
      if (!syncBtnEl.disabled) {
        syncBtnEl.textContent = "同步";
        syncBtnEl.title = "立即同步主机";
      }
    }, 2000);
  }
}

async function onLogoutClick() {
  try {
    state.login = await invoke("logout");
    renderLoginStatus();
    closeLoginModal();
  } catch (err) {
    uiAlert(`退出失败: ${err}`);
  }
}

loginBtnEl.addEventListener("click", openLoginModal);
$("#cancel-login-btn").addEventListener("click", closeLoginModal);
syncBtnEl.addEventListener("click", onSyncClick);
logoutBtnEl.addEventListener("click", onLogoutClick);

loginFormEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const f = loginFormEl.elements;
  const serverUrl = f.server_url.value.trim();
  const username = f.username.value.trim();
  const password = f.password.value;
  loginStatusEl.textContent = "登录中…";
  loginStatusEl.classList.remove("hidden");
  try {
    state.login = await invoke("login", { serverUrl, username, password });
    renderLoginStatus();
    logoutBtnEl.classList.remove("hidden");
    loginStatusEl.textContent = "登录成功";
    await loadHosts();
    await loadCertificates();
    updateMainView();
  } catch (err) {
    loginStatusEl.textContent = `登录失败: ${err}`;
  }
});

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
  // Groups are collapsed by default; keep user's explicit expand/collapse state.
  const seen = new Set();
  for (const host of state.hosts) {
    const cat = (host.catalog || "").trim() || "未分组";
    if (seen.has(cat)) continue;
    seen.add(cat);
    if (!(cat in state.collapsedGroups)) {
      state.collapsedGroups[cat] = true;
    }
  }
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

function makeItem({ selected, name, sub, active, onClick, actions, onContextMenu }) {
  const li = document.createElement("li");
  li.className = "item" + (selected ? " selected" : "");

  if (onContextMenu) {
    li.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      e.stopPropagation();
      onContextMenu(e);
    });
  }

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

function toggleGroup(cat) {
  state.collapsedGroups[cat] = !state.collapsedGroups[cat];
  renderHostList();
}

function renderHostList() {
  hostListEl.textContent = "";
  if (state.hosts.length === 0) {
    hostListEl.appendChild(makeEmptyItem("暂无主机"));
    return;
  }
  const q = (state.searchHosts || "").trim().toLowerCase();
  const hosts = q
    ? state.hosts.filter((h) =>
        [h.name, h.hostname, h.username, h.catalog]
          .filter(Boolean)
          .some((v) => String(v).toLowerCase().includes(q))
      )
    : state.hosts;
  if (hosts.length === 0) {
    hostListEl.appendChild(makeEmptyItem("无匹配主机"));
    return;
  }
  const groups = new Map();
  for (const host of hosts) {
    const cat = (host.catalog || "").trim() || "未分组";
    if (!groups.has(cat)) groups.set(cat, []);
    groups.get(cat).push(host);
  }
  const sorted = [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0], "zh"));
  for (const [cat, hosts] of sorted) {
    const collapsed = !!state.collapsedGroups[cat];

    const header = document.createElement("li");
    header.className = "list-group-item";

    const btn = document.createElement("button");
    btn.className = "list-group" + (collapsed ? " collapsed" : "");
    btn.type = "button";
    btn.title = collapsed ? "展开分组" : "收缩分组";

    const caret = document.createElement("span");
    caret.className = "group-caret";
    caret.innerHTML =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

    const label = document.createElement("span");
    label.className = "group-label";
    label.textContent = cat;

    const count = document.createElement("span");
    count.className = "group-count";
    count.textContent = hosts.length;

    btn.append(caret, label, count);
    btn.addEventListener("click", () => toggleGroup(cat));
    header.appendChild(btn);
    hostListEl.appendChild(header);

    if (collapsed) continue;

    hosts.sort((a, b) => (a.name || "").localeCompare(b.name || "", "zh"));
    for (const host of hosts) {
      hostListEl.appendChild(
        makeItem({
          selected: host.id === state.selectedHostId,
          name: host.name,
          sub: `${host.username}@${host.hostname}:${host.port}${host.is_public ? " · 公共" : ""}`,
          active: !!state.termId && host.id === state.connectedHostId,
          onClick: () => selectHost(host.id),
          onContextMenu: (e) => openHostContextMenu(e, host.id),
        })
      );
    }
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
  repoViewEl.classList.add("hidden");
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
  } else if (state.section === "proxy") {
    proxyViewEl.classList.remove("hidden");
    return;
  } else if (state.section === "repo") {
    repoViewEl.classList.remove("hidden");
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
  hideClusterFrame();

  state.selectedHostId = id;
  hostNameEl.textContent = host.name;
  hostConnEl.textContent = `${host.username}@${host.hostname}:${host.port}`;
  if (!state.hostSyncing) hideHostSyncProgress();
  renderHostList();
  updateMainView();
  updateInjectUI();
  renderClusterMgrBtn();
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
  $("#nav-repo").classList.toggle("active", section === "repo");
  hostSearchBoxEl.classList.toggle("hidden", section !== "hosts");
  hostListEl.classList.toggle("hidden", section !== "hosts");
  tunnelListEl.classList.toggle("hidden", section !== "tunnels");
  certListEl.classList.toggle("hidden", section !== "certificates");
  setListEl.classList.toggle("hidden", section !== "repo");
  proxySidebarEl.classList.toggle("hidden", section !== "proxy");
  sidebarAddRowEl.classList.toggle("hidden", section === "proxy");
  addBtnEl.textContent =
    section === "hosts"
      ? "+ 添加主机"
      : section === "tunnels"
        ? "+ 添加本地隧道"
        : section === "repo"
          ? "+ 添加软件集"
          : "+ 添加本地证书";
  updateSyncBtn();
  updateMainView();
  if (section === "repo") {
    enterRepo();
  }
}

// ---- software repository ----------------------------------------------------

function repoPathDisplay() {
  return "/" + (state.repoPath || "").replace(/^\/+|\/+$/g, "");
}

function formatSize(n) {
  if (!n) return "";
  return formatBytes(n);
}

function formatBytes(n) {
  const v = Number(n) || 0;
  if (v < 1024) return `${v} B`;
  if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB`;
  if (v < 1024 * 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)} MB`;
  return `${(v / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

function selectedSoftwareSet() {
  const name = state.selectedSetName || (state.repoStatus && state.repoStatus.setName) || "";
  return (state.softwareSets || []).find((s) => s.name === name) || null;
}

async function loadSoftwareSets() {
  state.softwareSets = (await invoke("list_software_sets")) || [];
  if (!state.selectedSetName) {
    const current = (state.repoStatus && state.repoStatus.setName) || "";
    state.selectedSetName =
      current || (state.softwareSets[0] && state.softwareSets[0].name) || "np4";
  }
}

function setKindLabel(kind) {
  return kind === "git" ? "Git" : "Manifest";
}

function renderSetList() {
  setListEl.textContent = "";
  if (!state.softwareSets.length) {
    setListEl.appendChild(makeEmptyItem("暂无软件集"));
    return;
  }
  for (const set of state.softwareSets) {
    const kind = setKindLabel(set.kind);
    setListEl.appendChild(
      makeItem({
        selected: set.name === state.selectedSetName,
        name: set.name,
        sub: set.cloned ? `${kind} · 已同步` : `${kind} · 未同步`,
        onClick: () => selectSoftwareSet(set.name),
        onContextMenu: (e) => openSetContextMenu(e, set.name),
      })
    );
  }
}

async function loadRepoStatus() {
  state.repoStatus = await invoke("repo_status");
  if (state.repoStatus && state.repoStatus.setName) {
    state.selectedSetName = state.repoStatus.setName;
  }
}

function hideRepoProgress() {
  repoProgressEl.classList.add("hidden");
  repoProgressBarEl.style.width = "0%";
  repoProgressPctEl.textContent = "0%";
  repoProgressLabelEl.textContent = "同步中";
  repoProgressDetailEl.textContent = "";
}

function renderRepoProgress(p) {
  repoProgressEl.classList.remove("hidden");
  const current = p.current || 0;
  const total = p.total || 0;
  const overallDone = Number(p.overallDone) || 0;
  const overallTotal = Number(p.overallTotal) || 0;
  const bytesDone = Number(p.bytesDone) || 0;
  const bytesTotal = Number(p.bytesTotal) || 0;
  let pct = 0;
  if (overallTotal > 0) {
    pct = Math.min(100, (overallDone / overallTotal) * 100);
  } else if (total > 0) {
    pct = Math.min(100, (current / total) * 100);
  }
  const git =
    p.action === "git" || p.action === "git-clone" || p.action === "git-fetch";
  const action =
    p.action === "download"
      ? "下载"
      : p.action === "skip"
        ? "跳过"
        : p.action === "fail"
          ? "失败"
          : p.action === "git-clone"
            ? "克隆"
            : p.action === "git-fetch"
              ? "拉取"
              : p.action === "git"
                ? "Git"
                : p.action || "同步";
  repoProgressLabelEl.textContent = git
    ? `Git ${action}中`
    : total > 0
      ? `同步中 ${current}/${total} · ${action}`
      : `同步中 · ${action}`;
  repoProgressPctEl.textContent = `${pct.toFixed(pct >= 10 ? 0 : 1)}%`;
  repoProgressBarEl.style.width = `${pct}%`;
  const filePart = p.file || "";
  const sizePart =
    (p.action === "download" || git) && (bytesDone > 0 || bytesTotal > 0)
      ? bytesTotal
        ? `${formatBytes(bytesDone)} / ${formatBytes(bytesTotal)}`
        : formatBytes(bytesDone)
      : "";
  repoProgressDetailEl.textContent = [filePart, sizePart].filter(Boolean).join(" · ");
}

function renderRepoStatus() {
  const s = state.repoStatus || {
    cloned: false,
    localPath: "",
    setName: state.selectedSetName || "",
    totalFiles: 0,
    downloaded: 0,
    skipped: 0,
    failed: 0,
    error: "",
  };
  const set = selectedSoftwareSet();
  const setName = s.setName || state.selectedSetName || "软件仓库";
  const kind = (set && set.kind) || s.kind || "manifest";
  repoSetTitleEl.textContent = setName;
  repoCloneUpdateBtnEl.textContent = state.repoSyncing ? "同步中…" : "同步";
  repoCloneUpdateBtnEl.disabled = state.repoSyncing || !setName;
  repoCloneUpdateBtnEl.className = "btn primary";
  repoPathEl.textContent = s.localPath || "同步后自动填充";
  repoPathEl.title = s.localPath || "";
  if (state.repoSyncing) {
    repoStatusLineEl.className = "conn";
  } else if (s.cloned) {
    if (kind === "git") {
      const ref = [s.branch || (set && set.branch), s.commit || (set && set.commit)]
        .filter(Boolean)
        .join(" · ");
      repoStatusLineEl.textContent = ref ? `Git 已同步 · ${ref}` : "Git 已同步";
    } else {
      const extra =
        s.totalFiles > 0
          ? ` · ${s.totalFiles} 文件 · 下载 ${s.downloaded} · 跳过 ${s.skipped}` +
            (s.failed ? ` · 失败 ${s.failed}` : "")
          : "";
      repoStatusLineEl.textContent = `Manifest 已同步${extra}`;
    }
    repoStatusLineEl.className = "conn";
  } else {
    repoStatusLineEl.textContent = kind === "git" ? "Git 未同步" : "Manifest 未同步";
    repoStatusLineEl.className = "conn";
  }
  repoUpBtnEl.disabled = state.repoSyncing || !s.cloned || !state.repoPath;
}

async function enterRepo() {
  await loadSoftwareSets();
  await loadRepoStatus();
  renderSetList();
  renderRepoStatus();
  if (!state.repoSyncing) hideRepoProgress();
  if (state.repoStatus && state.repoStatus.cloned) {
    await loadRepoDir();
  } else {
    state.repoPath = "";
    repoCurrentPathEl.textContent = "/";
    repoTreeEl.textContent = "";
    repoFileTitleEl.textContent = "同步软件集后可浏览目录和内容";
    repoFileContentEl.textContent = "";
  }
}

async function selectSoftwareSet(name) {
  if (!name || name === state.selectedSetName) {
    state.selectedSetName = name;
    renderSetList();
    return;
  }
  try {
    state.repoStatus = await invoke("select_software_set", { setName: name });
    state.selectedSetName = name;
    state.repoPath = "";
    renderSetList();
    renderRepoStatus();
    if (state.repoStatus && state.repoStatus.cloned) {
      await loadRepoDir();
    } else {
      repoCurrentPathEl.textContent = "/";
      repoTreeEl.textContent = "";
      repoFileTitleEl.textContent = "同步软件集后可浏览目录和内容";
      repoFileContentEl.textContent = "";
    }
  } catch (err) {
    uiAlert(`Error: ${err}`);
  }
}

function selectedSetKind() {
  const set = selectedSoftwareSet();
  return (set && set.kind) || (state.repoStatus && state.repoStatus.kind) || "manifest";
}

function updateSetModalKindFields() {
  const kind = (setFormEl.elements.kind.value || "manifest");
  const git = kind === "git";
  $("#set-git-fields").classList.toggle("hidden", !git);
  $("#set-manifest-hint").classList.toggle("hidden", git);
  setFormEl.elements.git_url.required = git;
}

function closeSetModal() {
  setModalEl.classList.add("hidden");
  state.editingSetName = null;
}

function openSetModal(set) {
  state.editingSetName = set ? set.name : null;
  setModalTitleEl.textContent = set ? "编辑软件集" : "添加软件集";
  const f = setFormEl.elements;
  f.name.value = set ? set.name : "";
  f.name.readOnly = !!set;
  const kind = set && set.kind === "git" ? "git" : "manifest";
  setFormEl.querySelectorAll('input[name="kind"]').forEach((radio) => {
    radio.checked = radio.value === kind;
    radio.disabled = !!set;
  });
  f.git_url.value = set ? set.gitUrl || "" : "";
  f.git_branch.value = set ? set.gitBranch || "" : "";
  f.git_username.value = set ? set.gitUsername || "" : "";
  f.git_token.value = set ? set.gitToken || "" : "";
  updateSetModalKindFields();
  setModalEl.classList.remove("hidden");
  f.name.focus();
}

async function onAddSoftwareSet() {
  openSetModal(null);
}

function onEditSoftwareSet(name) {
  const setName = name || state.contextSetName || state.selectedSetName;
  const set = (state.softwareSets || []).find((s) => s.name === setName);
  if (!set) {
    uiAlert("请先选择软件集");
    return;
  }
  openSetModal(set);
}

async function onDeleteSoftwareSet(name) {
  const setName = name || state.contextSetName || (selectedSoftwareSet() && selectedSoftwareSet().name) || state.selectedSetName;
  if (!setName) return;
  const ok = await uiConfirm(`删除软件集 ${setName}？本地已下载的文件也会一并删除。`, "删除软件集");
  if (!ok) return;
  try {
    state.softwareSets = (await invoke("remove_software_set", { setName })) || [];
    state.selectedSetName = "";
    state.repoPath = "";
    await enterRepo();
  } catch (err) {
    uiAlert(`Error: ${err}`);
  }
}

async function loadRepoDir() {
  if (!state.repoStatus || !state.repoStatus.cloned) return;
  try {
    const entries = await invoke("list_repo_files", { path: state.repoPath });
    renderRepoTree(entries);
    repoCurrentPathEl.textContent = repoPathDisplay();
    repoUpBtnEl.disabled = !state.repoPath;
  } catch (err) {
    repoTreeEl.textContent = "";
    repoCurrentPathEl.textContent = repoPathDisplay();
    repoFileTitleEl.textContent = `加载失败: ${err}`;
    repoFileContentEl.textContent = "";
  }
}

function renderRepoTree(entries) {
  repoTreeEl.textContent = "";
  if (!entries.length) {
    const li = document.createElement("li");
    li.className = "list-empty";
    li.textContent = "空目录";
    repoTreeEl.appendChild(li);
    return;
  }
  for (const entry of entries) {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.className = "repo-item";
    btn.type = "button";
    btn.title = entry.path;

    const icon = document.createElement("span");
    icon.className = "repo-item-icon";
    icon.textContent = entry.isDir ? "📁" : "📄";

    const name = document.createElement("span");
    name.className = "repo-item-name";
    name.textContent = entry.name;

    const size = document.createElement("span");
    size.className = "repo-item-size";
    size.textContent = entry.isDir ? "" : formatSize(entry.size);

    btn.append(icon, name, size);
    btn.addEventListener("click", () => {
      if (entry.isDir) {
        state.repoPath = entry.path;
        loadRepoDir();
      } else {
        openRepoFile(entry.path);
      }
    });
    li.appendChild(btn);
    repoTreeEl.appendChild(li);
  }
}

async function openRepoFile(path) {
  try {
    const file = await invoke("read_repo_file", { path });
    repoFileTitleEl.textContent = `/${file.path} · ${formatSize(file.size)}`;
    repoFileContentEl.textContent = file.content;
  } catch (err) {
    repoFileTitleEl.textContent = `/${path}`;
    repoFileContentEl.textContent = `无法预览: ${err}`;
  }
}

function repoUp() {
  if (!state.repoPath) return;
  const parts = state.repoPath.split("/").filter(Boolean);
  parts.pop();
  state.repoPath = parts.join("/");
  loadRepoDir();
}

async function onRepoCloneUpdateClick() {
  const setName = state.selectedSetName || (state.repoStatus && state.repoStatus.setName) || "";
  if (!setName) {
    uiAlert("请先选择软件集");
    return;
  }
  const kind = selectedSetKind();
  if (kind !== "git" && (!state.login || !state.login.serverUrl)) {
    uiAlert("请先登录维护中心，以便获取服务器地址");
    return;
  }
  state.repoSyncing = true;
  renderRepoProgress({
    current: 0,
    total: 0,
    file: "",
    action: "download",
    bytesDone: 0,
    bytesTotal: 0,
    overallDone: 0,
    overallTotal: 0,
  });
  renderRepoStatus();
  try {
    state.repoStatus = await invoke("sync_software_set", { setName });
    state.selectedSetName = setName;
    state.repoPath = "";
    await loadSoftwareSets();
    renderSetList();
    renderRepoStatus();
    if (state.repoStatus.cloned) {
      await loadRepoDir();
    }
    if (state.repoStatus.error) {
      uiAlert(`部分文件同步失败: ${state.repoStatus.error}`);
    }
  } catch (err) {
    uiAlert(`Error: ${err}`);
    await loadRepoStatus();
    renderRepoStatus();
  } finally {
    state.repoSyncing = false;
    hideRepoProgress();
    renderRepoStatus();
  }
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
  f.catalog.value = host ? host.catalog || "" : "";
  f.hostname.value = host ? host.hostname : "";
  f.port.value = host ? host.port : 22;
  f.username.value = host ? host.username : "";
  f.inject_remote_port.value = host ? hostInjectRemotePort(host) : 7890;
  f.is_public.checked = host ? !!host.is_public : false;
  f.is_public.disabled = false;

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

function openCopyHostModal(host) {
  if (!host) return;
  state.editingHostId = null;
  hostModalTitleEl.textContent = "复制主机";
  fillCertSelects();

  const f = hostFormEl.elements;
  f.name.value = `${host.name} 副本`;
  f.catalog.value = host.catalog || "";
  f.hostname.value = host.hostname;
  f.port.value = host.port;
  f.username.value = host.username;
  f.inject_remote_port.value = hostInjectRemotePort(host);
  f.is_public.checked = false;
  f.is_public.disabled = false;

  if (host.auth && host.auth.method === "certificate") {
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
  f.name.select();
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
  if (!(await uiConfirm(`Delete host "${host.name}"?`))) return;
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
    uiAlert(`Error: ${err}`);
  }
}

async function deleteSelectedTunnel() {
  const t = tunnelById(state.selectedTunnelId);
  if (!t) return;
  if (!(await uiConfirm(`Delete tunnel "${t.name}"?`))) return;
  try {
    await invoke("delete_tunnel", { id: t.id });
    state.selectedTunnelId = null;
    await loadTunnels();
    updateMainView();
  } catch (err) {
    uiAlert(`Error: ${err}`);
  }
}

async function deleteSelectedCert() {
  const cert = certById(state.selectedCertId);
  if (!cert) return;
  if (!(await uiConfirm(`Delete certificate "${cert.name}"?`))) return;
  try {
    await invoke("delete_certificate", { id: cert.id });
    state.selectedCertId = null;
    await loadCertificates();
    updateMainView();
  } catch (err) {
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Parse failed: ${err}`);
  }
}

// ---- events -----------------------------------------------------------------

$("#nav-hosts").addEventListener("click", () => switchSection("hosts"));
hostSearchInputEl.addEventListener("input", () => {
  state.searchHosts = hostSearchInputEl.value;
  renderHostList();
});
$("#nav-tunnels").addEventListener("click", () => switchSection("tunnels"));
$("#nav-certificates").addEventListener("click", () => switchSection("certificates"));
$("#nav-proxy").addEventListener("click", () => switchSection("proxy"));
$("#nav-repo").addEventListener("click", () => switchSection("repo"));
$("#proxy-indicator").addEventListener("click", () => switchSection("proxy"));
repoCloneUpdateBtnEl.addEventListener("click", onRepoCloneUpdateClick);
if (setFormEl) {
  setFormEl.addEventListener("submit", async (e) => {
    e.preventDefault();
    const f = setFormEl.elements;
    const setName = f.name.value.trim();
    const kind = f.kind.value || "manifest";
    if (!setName) {
      uiAlert("请填写软件集名称");
      return;
    }
    if (kind === "git" && !f.git_url.value.trim()) {
      uiAlert("请填写 Git 仓库地址");
      return;
    }
    const payload = {
      setName,
      kind,
      gitUrl: f.git_url.value.trim(),
      gitUsername: f.git_username.value.trim(),
      gitToken: f.git_token.value,
      gitBranch: f.git_branch.value.trim(),
    };
    try {
      const cmd = state.editingSetName ? "update_software_set" : "add_software_set";
      state.softwareSets = (await invoke(cmd, payload)) || [];
      state.selectedSetName = setName;
      state.repoPath = "";
      closeSetModal();
      await loadRepoStatus();
      renderSetList();
      renderRepoStatus();
      if (state.repoStatus && state.repoStatus.cloned) {
        await loadRepoDir();
      } else {
        repoCurrentPathEl.textContent = "/";
        repoTreeEl.textContent = "";
        repoFileTitleEl.textContent = "同步软件集后可浏览目录和内容";
        repoFileContentEl.textContent = "";
      }
    } catch (err) {
      uiAlert(`Error: ${err}`);
    }
  });
  setFormEl.querySelectorAll('input[name="kind"]').forEach((el) => {
    el.addEventListener("change", updateSetModalKindFields);
  });
}
if ($("#cancel-set-btn")) {
  $("#cancel-set-btn").addEventListener("click", closeSetModal);
}
repoUpBtnEl.addEventListener("click", repoUp);

addBtnEl.addEventListener("click", () => {
  if (state.section === "hosts") openHostModal(null);
  else if (state.section === "tunnels") openTunnelModal(null);
  else if (state.section === "certificates") openCertModal();
  else if (state.section === "repo") onAddSoftwareSet();
});

$("#cancel-host-btn").addEventListener("click", closeHostModal);

$("#edit-tunnel-btn").addEventListener("click", () => {
  const t = tunnelById(state.selectedTunnelId);
  if (t) openTunnelModal(t);
});
$("#delete-tunnel-btn").addEventListener("click", deleteSelectedTunnel);
$("#cancel-tunnel-btn").addEventListener("click", closeTunnelModal);
$("#parse-btn").addEventListener("click", parseSshCommand);

$("#delete-cert-btn").addEventListener("click", deleteSelectedCert);
$("#cancel-cert-btn").addEventListener("click", closeCertModal);

document.querySelectorAll(".modal-close").forEach((btn) => {
  btn.addEventListener("click", () => {
    const el = document.getElementById(btn.dataset.close);
    if (el) el.classList.add("hidden");
  });
});

// ---- context menu ------------------------------------------------------------

// Disable the browser's default right-click menu on the page (keep it for
// form fields so users can still copy/paste while editing).
document.addEventListener("contextmenu", (e) => {
  const editable = e.target && e.target.closest
    ? e.target.closest("input, textarea, select, [contenteditable]")
    : null;
  if (!editable) e.preventDefault();
});

function hideContextMenu() {
  state.contextHostId = null;
  state.contextSetName = null;
  ctxMenuEl.classList.add("hidden");
  if (setCtxMenuEl) setCtxMenuEl.classList.add("hidden");
}

function placeContextMenu(el, e) {
  el.classList.remove("hidden");
  const menuW = el.offsetWidth || 160;
  const menuH = el.offsetHeight || 40;
  const pad = 8;
  const x = Math.min(e.clientX, window.innerWidth - menuW - pad);
  const y = Math.min(e.clientY, window.innerHeight - menuH - pad);
  el.style.left = `${Math.max(pad, x)}px`;
  el.style.top = `${Math.max(pad, y)}px`;
}

function openSetContextMenu(e, setName) {
  state.contextSetName = setName;
  if (ctxMenuEl) ctxMenuEl.classList.add("hidden");
  if (!setCtxMenuEl) return;
  placeContextMenu(setCtxMenuEl, e);
}

function openHostContextMenu(e, hostId) {
  const host = hostById(hostId);
  state.contextHostId = hostId;

  const canManage = !!(host && host.owned);
  ctxEditHostBtnEl.classList.toggle("hidden", !canManage);
  ctxDeleteHostBtnEl.classList.toggle("hidden", !canManage);

  if (setCtxMenuEl) setCtxMenuEl.classList.add("hidden");
  placeContextMenu(ctxMenuEl, e);
}

ctxEditHostBtnEl.addEventListener("click", () => {
  const host = hostById(state.contextHostId);
  hideContextMenu();
  if (host && host.owned) openHostModal(host);
});

ctxCopyHostBtnEl.addEventListener("click", () => {
  const host = hostById(state.contextHostId);
  hideContextMenu();
  if (host) openCopyHostModal(host);
});

ctxDeleteHostBtnEl.addEventListener("click", () => {
  const host = hostById(state.contextHostId);
  hideContextMenu();
  if (host && host.owned) deleteHost(host);
});

if (ctxEditSetBtnEl) {
  ctxEditSetBtnEl.addEventListener("click", () => {
    const name = state.contextSetName;
    hideContextMenu();
    if (name) onEditSoftwareSet(name);
  });
}
if (ctxDeleteSetBtnEl) {
  ctxDeleteSetBtnEl.addEventListener("click", () => {
    const name = state.contextSetName;
    hideContextMenu();
    if (name) onDeleteSoftwareSet(name);
  });
}

document.addEventListener("click", hideContextMenu);
document.addEventListener("scroll", hideContextMenu, true);
window.addEventListener("resize", hideContextMenu);
window.addEventListener("blur", hideContextMenu);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideContextMenu();
});

async function onCheckEnvClick() {
  const host = hostById(state.selectedHostId);
  if (!host) return;
  const btn = checkEnvBtnEl;
  btn.disabled = true;
  btn.textContent = "检查中…";
  try {
    const result = await invoke("check_host_env", { hostId: host.id });
    btn.textContent = result.changed ? "已开启转发" : "环境正常";
    btn.title = result.message || `AllowTcpForwarding: ${result.allowTcpForwarding}`;
  } catch (err) {
    btn.textContent = "检查失败";
    btn.title = String(err);
    uiAlert(`检查环境失败: ${err}`);
  } finally {
    btn.disabled = false;
    setTimeout(() => {
      if (!btn.disabled) {
        btn.textContent = "检查环境";
        btn.title = "检查并修改 sshd 允许 TCP Forward";
      }
    }, 3000);
  }
}

async function onSoftwareSyncClick() {
  const host = hostById(state.selectedHostId);
  if (!host || state.hostSyncing) return;
  const ok = await uiConfirm(
    `将本地已拉取的软件同步到主机「${host.name}」的软件仓库目录（cangling-update 程序目录下的 repo/）。继续？`,
    "软件同步"
  );
  if (!ok) return;
  state.hostSyncing = true;
  renderSoftwareSyncBtn();
  renderHostSyncProgress({
    current: 0,
    total: 0,
    file: "",
    action: "upload",
    bytesDone: 0,
    bytesTotal: 0,
    overallDone: 0,
    overallTotal: 0,
    remotePath: "",
  });
  try {
    const result = await invoke("sync_host_software", { hostId: host.id });
    hideHostSyncProgress();
    const extra =
      result && result.totalFiles
        ? `上传 ${result.uploaded} · 跳过 ${result.skipped}` +
          (result.failed ? ` · 失败 ${result.failed}` : "")
        : "";
    const path = result && result.remotePath ? `\n远端：${result.remotePath}` : "";
    if (result && result.error) {
      uiAlert(`部分文件同步失败: ${result.error}${path}`);
    } else {
      uiAlert(`软件已同步到 Master 软件仓库。${extra}${path}`, "软件同步");
    }
  } catch (err) {
    hideHostSyncProgress();
    uiAlert(`软件同步失败: ${err}`);
  } finally {
    state.hostSyncing = false;
    renderSoftwareSyncBtn();
  }
}

async function onResourceMgrClick() {
  const host = hostById(state.selectedHostId);
  if (!host || state.clusterConnecting) return;
  const open =
    clusterFrameEl &&
    !clusterFrameEl.classList.contains("hidden") &&
    state.clusterForwardHostId === host.id;
  if (open) {
    hideClusterFrame();
    return;
  }
  const gen = ++state.clusterConnectGen;
  state.clusterConnecting = true;
  renderClusterMgrBtn();
  try {
    const info = await invoke("connect_update_console", { hostId: host.id });
    if (gen !== state.clusterConnectGen || state.selectedHostId !== host.id) {
      invoke("disconnect_update_console", { hostId: host.id }).catch(() => {});
      return;
    }
    const remotePort = Number(info && info.remotePort) || 0;
    if (remotePort > 0) state.clusterPorts[host.id] = remotePort;
    state.clusterForwardHostId = host.id;
    state.clusterLocalPort = Number(info.localPort) || 0;
    state.clusterConnecting = false;
    showClusterFrame(info.url, remotePort);
  } catch (err) {
    if (gen === state.clusterConnectGen) {
      state.clusterConnecting = false;
      renderClusterMgrBtn();
      uiAlert(`连接集群控制台失败: ${err}`);
    }
  }
}

async function onClusterFrameExternal() {
  const url = state.clusterFrameUrl || clusterMgrUrl(hostById(state.selectedHostId));
  if (!url) return;
  try {
    await invoke("open_url", { url });
  } catch (err) {
    uiAlert(`打开集群管理失败: ${err}`);
  }
}

toggleTunnelBtnEl.addEventListener("click", toggleTunnel);
termToggleBtnEl.addEventListener("click", toggleTerminal);
checkEnvBtnEl.addEventListener("click", onCheckEnvClick);
resourceMgrBtnEl.addEventListener("click", onResourceMgrClick);
if (clusterFrameCloseEl) clusterFrameCloseEl.addEventListener("click", hideClusterFrame);
if (clusterFrameRefreshEl) {
  clusterFrameRefreshEl.addEventListener("click", () => {
    const url = state.clusterFrameUrl || clusterMgrUrl(hostById(state.selectedHostId));
    if (url && clusterIframeEl) clusterIframeEl.src = url;
  });
}
if (clusterFrameExternalEl) {
  clusterFrameExternalEl.addEventListener("click", onClusterFrameExternal);
}
if (softwareSyncBtnEl) {
  softwareSyncBtnEl.addEventListener("click", onSoftwareSyncClick);
}
injectBtnEl.addEventListener("click", toggleInject);
updateBtnEl.addEventListener("click", onCanglingUpdateClick);
roleSwitchEl.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-role]");
  if (!btn || btn.disabled) return;
  onRoleSwitchClick(btn.dataset.role);
});
clusterTokenCopyEl.addEventListener("click", copyClusterToken);
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
    catalog: f.catalog.value.trim(),
    is_public: f.is_public.checked,
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
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Error: ${err}`);
  }
});

listen("tunnel-stopped", async () => {
  await loadTunnels();
  renderTunnelDetail();
});

listen("host-software-sync-progress", (e) => {
  const p = e.payload || {};
  if (p.hostId && state.selectedHostId && p.hostId !== state.selectedHostId) return;
  state.hostSyncing = true;
  renderHostSyncProgress(p);
  renderSoftwareSyncBtn();
});

listen("repo-sync-progress", (e) => {
  const p = e.payload || {};
  if (p.setName && state.selectedSetName && p.setName !== state.selectedSetName) return;
  state.repoSyncing = true;
  renderRepoProgress(p);
  const git =
    p.action === "git" || p.action === "git-clone" || p.action === "git-fetch";
  const action =
    p.action === "download"
      ? "下载"
      : p.action === "skip"
        ? "跳过"
        : p.action === "fail"
          ? "失败"
          : p.action === "git-clone"
            ? "克隆"
            : p.action === "git-fetch"
              ? "拉取"
              : p.action === "git"
                ? "Git"
                : p.action || "";
  const bytes =
    (p.action === "download" || git) && (p.bytesDone || p.bytesTotal)
      ? p.bytesTotal
        ? ` · ${formatBytes(p.bytesDone || 0)} / ${formatBytes(p.bytesTotal)}`
        : ` · ${formatBytes(p.bytesDone || 0)}`
      : "";
  repoStatusLineEl.textContent = git
    ? `Git ${action}中 ${p.current || 0}% · ${p.file || ""}${bytes}`
    : `同步中 ${p.current || 0}/${p.total || 0} · ${action} ${p.file || ""}${bytes}`;
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

listen("update-console", (e) => {
  const info = e.payload;
  if (!info || info.active || !info.hostId) return;
  if (state.clusterForwardHostId !== info.hostId) return;
  const localPort = Number(info.localPort) || 0;
  if (localPort && state.clusterLocalPort && localPort !== state.clusterLocalPort) return;
  hideClusterFrame();
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
      uiAlert(status.message || "Proxy started but probe failed");
    }
  } catch (err) {
    uiAlert(`Error: ${err}`);
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
      uiAlert(status.message || "Existing proxy is not usable");
    }
  } catch (err) {
    uiAlert(`Error: ${err}`);
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
    uiAlert(`Error: ${err}`);
  } finally {
    btn.disabled = false;
  }
}

async function stopProxy() {
  try {
    const status = await invoke("stop_proxy");
    applyProxyStatus(status);
  } catch (err) {
    uiAlert(`Error: ${err}`);
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

// ---- custom dialog (alert / confirm / prompt) --------------------------------

const uiDialogEl = $("#ui-dialog");
const uiDialogTitleEl = $("#ui-dialog-title");
const uiDialogMessageEl = $("#ui-dialog-message");
const uiDialogInputEl = $("#ui-dialog-input");
const uiDialogCancelBtnEl = $("#ui-dialog-cancel");
const uiDialogOkBtnEl = $("#ui-dialog-ok");
const uiDialogCloseBtnEl = $("#ui-dialog-close");

let uiDialogResolver = null;
let uiDialogMode = "alert"; // "alert" | "confirm" | "prompt"

function uiDialogOpen({ title, message, mode, defaultValue }) {
  uiDialogMode = mode;
  uiDialogTitleEl.textContent = title;
  uiDialogMessageEl.textContent = message;
  uiDialogInputEl.classList.toggle("hidden", mode !== "prompt");
  uiDialogCancelBtnEl.classList.toggle("hidden", mode === "alert");
  uiDialogCloseBtnEl.classList.toggle("hidden", mode === "alert");
  uiDialogInputEl.value = defaultValue == null ? "" : String(defaultValue);
  uiDialogEl.classList.remove("hidden");
  if (mode === "prompt") {
    uiDialogInputEl.focus();
    uiDialogInputEl.select();
  } else {
    uiDialogOkBtnEl.focus();
  }
}

function uiDialogClose(value) {
  uiDialogEl.classList.add("hidden");
  const resolve = uiDialogResolver;
  uiDialogResolver = null;
  if (resolve) resolve(value);
}

function uiDialogOk() {
  if (uiDialogMode === "confirm") uiDialogClose(true);
  else if (uiDialogMode === "prompt") uiDialogClose(uiDialogInputEl.value);
  else uiDialogClose(undefined);
}

function uiDialogCancel() {
  if (uiDialogMode === "prompt") uiDialogClose(null);
  else if (uiDialogMode === "confirm") uiDialogClose(false);
  else uiDialogClose(undefined);
}

function showDialog(options) {
  // Defensively settle a dialog that is somehow already open.
  if (uiDialogResolver) {
    const prev = uiDialogResolver;
    uiDialogResolver = null;
    prev(null);
  }
  return new Promise((resolve) => {
    uiDialogResolver = resolve;
    uiDialogOpen(options);
  });
}

async function uiAlert(message, title = "提示") {
  await showDialog({ title, message: String(message ?? ""), mode: "alert" });
}

async function uiConfirm(message, title = "确认") {
  return await showDialog({ title, message: String(message ?? ""), mode: "confirm" });
}

async function uiPrompt(message, defaultValue = "", title = "输入") {
  return await showDialog({
    title,
    message: String(message ?? ""),
    mode: "prompt",
    defaultValue,
  });
}

uiDialogOkBtnEl.addEventListener("click", uiDialogOk);
uiDialogCancelBtnEl.addEventListener("click", uiDialogCancel);
uiDialogCloseBtnEl.addEventListener("click", uiDialogCancel);
uiDialogInputEl.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    uiDialogOk();
  }
});
uiDialogEl.addEventListener("mousedown", (e) => {
  if (e.target === uiDialogEl) uiDialogCancel();
});
document.addEventListener("keydown", (e) => {
  if (uiDialogEl.classList.contains("hidden")) return;
  if (e.key === "Escape") {
    e.preventDefault();
    uiDialogCancel();
  }
});

// ---- init -------------------------------------------------------------------

(async () => {
  await ensureTerminalFont();
  initTerminal();
  updateTerminalUI();
  checkAppUpdate();
  await loadLoginStatus();
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
    uiAlert(`Failed to load data: ${err}`);
  }

  // If the user is already logged in, sync the public host list in the
  // background so shared hosts and deletions are reflected automatically.
  autoSyncIfLoggedIn();
})();
