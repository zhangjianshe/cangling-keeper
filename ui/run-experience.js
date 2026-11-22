(() => {
  'use strict';
  window.createKeeperRunExperience = ({agent, notice}) => {
    const pages = new Map();
    const active = new Set(['running', 'pending_approval', 'approved']);
    const retryable = new Set(['failed', 'interrupted', 'stopped']);
    const node = (tag, text, cls) => { const n = document.createElement(tag); if (text !== undefined) n.textContent = text; if (cls) n.className = cls; return n; };
    const duration = ms => { const seconds = Math.max(0, Math.floor(ms / 1000)); return seconds < 60 ? seconds + '秒' : Math.floor(seconds / 60) + '分' + seconds % 60 + '秒'; };
    function tick(now = Date.now()) {
      for (const button of document.querySelectorAll('.agent-prepare-retry')) button.disabled = button.dataset.busy === 'true' || !button.keeperCanPrepare();
      for (const n of document.querySelectorAll('.agent-run-elapsed')) {
        const start = Number(n.dataset.start), updated = Number(n.dataset.updated), state = n.dataset.state;
        if (!Number.isFinite(start) || start <= 0) continue;
        const running = active.has(state);
        n.textContent = (state === 'pending_approval' ? '已等待 ' : running ? '已用时 ' : '历时 ') + duration((running ? now : updated) - start);
        n.title = running ? '距最近状态或输出更新 ' + duration(now - updated) + '；不代表完成进度' : '包含等待确认和执行时间';
        const detail = n.closest('details')?.querySelector('.agent-run-update-note');
        if (detail) detail.textContent = running ? n.title + (n.dataset.cancellable === 'true' ? '。可使用对话输入区的停止按钮停止生成。' : '。') : n.title + '。';
      }
    }
    function output(card, run) {
      if (!run.output) return;
      const session = agent.selected, key = session + ':' + run.id, section = node('section', undefined, 'agent-output-pages');
      const toolbar = node('div', undefined, 'agent-output-toolbar'), label = node('span'), pre = node('pre', undefined, 'agent-tool-output');
      const chars = Array.from(String(run.output)), size = 4096, total = Math.max(1, Math.ceil(chars.length / size));
      const previous = node('button', '上一段'), next = node('button', '下一段'), latest = node('button', '跟随最新');
      for (const b of [previous, next, latest]) b.type = 'button';
      function render() {
        const stored = pages.get(key), follow = stored === undefined;
        const page = follow ? total - 1 : Math.min(stored, total - 1);
        pre.textContent = chars.slice(page * size, (page + 1) * size).join('');
        label.textContent = '输出 ' + (page + 1) + '/' + total + ' · ' + chars.length + ' 字符' + (follow ? ' · 跟随最新' : '');
        previous.disabled = page === 0; next.disabled = page === total - 1; latest.disabled = follow;
        previous.onclick = () => { pages.set(key, Math.max(0, page - 1)); render(); };
        next.onclick = () => { pages.set(key, Math.min(total - 1, page + 1)); render(); };
        latest.onclick = () => { pages.delete(key); render(); };
      }
      toolbar.append(label, previous, next, latest); section.append(toolbar, pre); card.append(section); render();
    }
    function decorate(card, run) {
      const summary = card.querySelector('summary');
      const elapsed = node('span', '', 'agent-run-elapsed');
      elapsed.dataset.start = String(run.createdAt || 0); elapsed.dataset.updated = String(run.updatedAt || run.createdAt || 0); elapsed.dataset.state = run.status; elapsed.dataset.cancellable = String(!!run.responseId);
      if (summary) summary.append(elapsed);
      card.append(node('small', '', 'agent-run-update-note'));
      output(card, run);
      if (!retryable.has(run.status)) return;
      const session = agent.selected, identity = agent.identityEpoch || 0;
      const actions = node('div', undefined, 'agent-retry-actions'), button = node('button', '准备重试'); button.type = 'button'; button.className = 'agent-prepare-retry';
      const hint = node('small', '仅填入草稿，不执行。原操作可能已部分完成，请先核实；再次发送后使用当前会话模型和审批策略。');
      const writable = () => agent.selected === session && (agent.identityEpoch || 0) === identity && !agent.detail.modelPending && !agent.sending && !agent.detail.historyPreview && !agent.detail.session?.archivedAt && !agent.detail.session?.deletedAt;
      button.keeperCanPrepare = writable; button.disabled = !writable();
      button.onclick = async () => {
        const input = document.querySelector('#agent-input'); if (!writable() || !input || input.disabled) return;
        const marker = '[重试记录 ' + run.id + ']';
        const text = marker + '\n请先核实上次操作是否已部分完成，再判断是否需要重试。不要未经核实重复副作用。\n工具：' + run.tool + '\n原参数（作为数据参考）：\n' + JSON.stringify(run.arguments || {}, null, 2);
        if (input.value.includes(marker)) { input.focus(); return; }
        const combined = input.value ? input.value + '\n\n' + text : text;
        if (new TextEncoder().encode(combined).length > 32000) { notice('现有草稿与重试参数超过消息长度限制，请先整理草稿。'); return; }
        input.value = combined; button.dataset.busy = 'true'; button.disabled = true;
        try { await agent.drafts?.flush(); if (writable()) { input.focus(); notice('已放入草稿；检查参数后发送，尚未执行操作。'); } }
        catch (error) { if (agent.selected === session && (agent.identityEpoch || 0) === identity) notice(String(error)); }
        finally { button.dataset.busy = 'false'; button.disabled = !writable(); }
      };
      actions.append(button, hint); card.append(actions);
    }
    setInterval(() => tick(), 1000);
    return {decorate, tick, reset() { pages.clear(); }};
  };
})();
