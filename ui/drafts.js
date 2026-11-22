(() => {
  window.createKeeperDrafts = ({agent, invoke, notice, ensureSession}) => {
    const input = document.querySelector('#agent-input');
    let current = null, navigation = 0, creating = null, sending = false;
    const identity = () => agent.identityEpoch || 0;
    function flush() {
      const state = current;
      if (!state) return Promise.resolve();
      const content = input.value;
      state.queue = state.queue.then(async () => {
        if (state.identity !== identity() || content === state.saved) return;
        const data = await invoke('agent_draft_save', {sessionId: state.id, content, revision: state.revision});
        state.revision = data.revision;
        state.saved = content;
      });
      // Keep a rejected queue blocked: never overwrite a conflicting tab's draft.
      return state.queue;
    }
    async function load(id) {
      const ticket = ++navigation;
      if (current?.id === id) { if (!sending) input.disabled = false; return true; }
      if (sending) throw new Error('消息正在保存，请稍后切换会话。');
      const owner = identity();
      input.disabled = true;
      try {
        if (creating) await creating;
        await flush();
        const data = id ? await invoke('agent_draft_get', {sessionId: id}) : {content: '', revision: 0};
        if (ticket !== navigation || owner !== identity()) return false;
        current = id ? {id, identity: owner, saved: data.content, revision: data.revision, queue: Promise.resolve()} : null;
        input.value = data.content;
        return true;
      } finally { if (ticket === navigation) input.disabled = false; }
    }
    async function establish() {
      if (current) return;
      if (!creating) {
        const owner = identity();
        creating = (async () => {
          const id = await ensureSession();
          const data = await invoke('agent_draft_get', {sessionId: id});
          if (owner !== identity()) throw new Error('账号已切换');
          if (!current) current = {id, identity: owner, saved: data.content, revision: data.revision, queue: Promise.resolve()};
        })().finally(() => { creating = null; });
      }
      await creating;
    }
    input.addEventListener('input', async () => {
      try { await establish(); await flush(); } catch (error) { notice(String(error)); }
    });
    window.addEventListener('beforeunload', event => {
      if (input.value !== (current?.saved || '')) { event.preventDefault(); event.returnValue = ''; }
    });
    return {
      flush, load,
      reset() { ++navigation; current = null; creating = null; sending = false; input.disabled = false; },
      async send(action) {
        sending = true; input.disabled = true;
        const owner = identity();
        try {
          await establish(); await flush();
          const state = current, text = input.value;
          await action(state.revision);
          if (owner !== identity() || state !== current) return;
          state.saved = ''; state.revision++;
          if (input.value === text) input.value = '';
          else await flush();
        } finally { if (owner === identity()) { sending = false; input.disabled = false; } }
      }
    };
  };
})();
