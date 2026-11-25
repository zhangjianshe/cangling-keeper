(() => {
  'use strict';
  const shell = document.getElementById('agent-shell');
  const button = document.getElementById('agent-sidebar-toggle');
  const content = document.getElementById('agent-sidebar-content');
  const key = 'keeper-sidebar-collapsed';
  function apply(collapsed) {
    shell.classList.toggle('agent-sidebar-collapsed', collapsed);
    content.hidden = collapsed;
    button.setAttribute('aria-expanded', String(!collapsed));
    button.title = collapsed ? '展开侧栏' : '收起侧栏';
    button.setAttribute('aria-label', button.title);
    window.dispatchEvent(new Event('resize'));
  }
  let collapsed = false;
  try { collapsed = localStorage.getItem(key) === 'true'; } catch (_) {}
  apply(collapsed);
  button.addEventListener('click', () => {
    collapsed = !collapsed;
    apply(collapsed);
    try { localStorage.setItem(key, String(collapsed)); } catch (_) {}
  });
})();
