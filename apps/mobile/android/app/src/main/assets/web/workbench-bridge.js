(() => {
  const host = window.WingmateWorkbench;
  if (!host || window.top !== window || location.origin !== __WINGMATE_ORIGIN__ || window.wingmateOpenSession) return;

  const send = (event, details = {}) => host.postMessage(JSON.stringify({ event, ...details }));
  const rows = '[data-row-key^="session:"]';
  const settingsCss = 'style[data-plugin-css="@deepseek-ai/dsh-client-ui-settings-general/SettingsRoot.module.css"]';
  const sessionId = row => row.dataset.rowKey.slice('session:'.length);
  let theme;
  let focusTimer;

  function publishTheme() {
    const source = document.documentElement.dataset.dsThemeSource;
    const next = ['light', 'dark', 'system'].includes(source)
      ? source : document.body.hasAttribute('data-ds-dark-theme') ? 'dark' : 'light';
    if (next === theme) return;
    theme = next;
    send('theme', { theme });
  }

  // Observe the DSH workbench's existing transport. The sidebar may be closed
  // while a task runs and its rows may never mount.
  const running = new Set();
  const waiting = new Set();
  function readStatus(message) {
    if (typeof message.data !== 'string') return;
    let frame;
    try { frame = JSON.parse(message.data); } catch { return; }
    const value = frame?.type === 'item' && frame.value;
    if (!value) return;
    if (value.type === 'waterfall' && value.eventId && value.agentId) {
      waiting.add(value.agentId);
    } else if (value.type === 'emit' && Array.isArray(value.args)) {
      const [id, active] = value.args;
      if (value.event === 'api-session/removed') {
        running.delete(id);
        waiting.delete(id);
      }
      if (value.event !== 'api-session/status' || typeof id !== 'string' || typeof active !== 'boolean') return;
      if (active) {
        running.add(id);
        waiting.delete(id);
      } else {
        const wasRunning = running.delete(id);
        const needsInput = waiting.delete(id);
        if (!wasRunning || needsInput) return;
        const row = Array.from(document.querySelectorAll(rows)).find(item => sessionId(item) === id);
        const title = row?.children[1]?.textContent.trim();
        send('completed', { sessionId: id, title: title?.slice(0, 256) });
      }
    }
  }
  window.WebSocket = new Proxy(window.WebSocket, {
    construct(target, args, type) {
      const socket = Reflect.construct(target, args, type);
      const endpoint = new URL(socket.url);
      const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
      if (endpoint.protocol === protocol && endpoint.host === location.host && endpoint.pathname === '/api/remote.mux') {
        socket.addEventListener('message', readStatus);
        socket.addEventListener('close', () => {
          running.clear();
          waiting.clear();
        });
      }
      return socket;
    },
  });

  window.wingmateOpenSession = id => {
    clearTimeout(focusTimer);
    const expires = Date.now() + 15000;
    let openedSidebar = false;
    const open = () => {
      const row = Array.from(document.querySelectorAll(rows)).find(item => sessionId(item) === id);
      if (row) {
        row.click();
        return;
      }
      const opener = document.querySelector('[data-sidebar-opener]');
      if (opener && !openedSidebar) {
        openedSidebar = true;
        opener.click();
      }
      if (Date.now() < expires) focusTimer = setTimeout(open, 250);
      else send('session-unavailable');
    };
    open();
  };

  function attachAppearance() {
    const appearance = document.createElement('style');
    appearance.textContent = `
      :root {
        --dsh-mobile-safe-area-inset-top: 0px;
        --dsh-mobile-safe-area-inset-bottom: 0px;
      }
      * { -webkit-tap-highlight-color: transparent; }
    `;
    document.head.append(appearance);

    if (!matchMedia('(width >= 0px)').matches) {
      const adapt = () => {
        const style = document.head.querySelector(settingsCss);
        if (!style) return;
        const css = style.textContent.replace(/@media\s*\(\s*width\s*<=\s*767px\s*\)/g, '@media (max-width: 767px)');
        if (css !== style.textContent) style.textContent = css;
      };
      new MutationObserver(adapt).observe(document.head, { childList: true });
      adapt();
    }

    const observer = new MutationObserver(publishTheme);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-ds-theme-source'] });
    observer.observe(document.body, { attributes: true, attributeFilter: ['data-ds-dark-theme'] });
    publishTheme();
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', attachAppearance, { once: true });
  else attachAppearance();
})();
