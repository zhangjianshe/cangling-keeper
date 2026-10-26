// Browser transport for the standalone `cangling-keeper --web` mode.  Tauri
// injects its own API before this script, so the desktop edition is untouched.
(() => {
  if (window.__TAURI__) return;

  const listeners = new Map();
  let socket;
  let retry;

  function connectEvents() {
    clearTimeout(retry);
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    socket = new WebSocket(`${scheme}//${location.host}/api/events`);
    socket.onmessage = (message) => {
      try {
        const event = JSON.parse(message.data);
        for (const callback of listeners.get(event.event) || []) {
          callback({ event: event.event, payload: event.payload });
        }
      } catch (_) {}
    };
    socket.onclose = () => {
      retry = setTimeout(connectEvents, 1000);
    };
  }

  async function invoke(command, args = {}) {
    if (command === "write_clipboard_text") {
      await navigator.clipboard.writeText(String(args.text || ""));
      return null;
    }
    if (command === "open_url") {
      window.open(args.url, "_blank", "noopener");
      return null;
    }
    const response = await fetch(`/api/invoke/${encodeURIComponent(command)}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(args || {}),
    });
    const body = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
    if (!response.ok || body.ok === false) throw body.error || `HTTP ${response.status}`;
    return body.result;
  }

  async function listen(event, callback) {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event).add(callback);
    return () => listeners.get(event)?.delete(callback);
  }

  window.__TAURI__ = { core: { invoke }, event: { listen } };
  connectEvents();
})();
