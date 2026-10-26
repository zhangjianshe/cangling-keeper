// Browser transport for the cangling-keeper web service.
(() => {
  if (window.__KEEPER__) return;

  const listeners = new Map();
  let socket;
  let retry;
  let stopped = false;

  function connectEvents() {
    if (stopped) return;
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
      if (!stopped) retry = setTimeout(connectEvents, 1000);
    };
  }

  function shutdownTransport() {
    stopped = true;
    clearTimeout(retry);
    if (socket) socket.close();
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

  window.__KEEPER__ = { core: { invoke }, event: { listen }, shutdownTransport };
  connectEvents();
})();
