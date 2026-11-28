([origin, theme, platform, transport]) => {
  let header = document.querySelector('[data-wingmate-remote-header]');
  if (!header) {
    header = document.createElement('div');
    header.dataset.wingmateRemoteHeader = '';
    const shadow = header.attachShadow({ mode: 'open' });
    const style = new CSSStyleSheet();
    style.replaceSync(`
      :host {
        position: fixed; inset: 0 0 auto; z-index: 1099; height: 40px;
        background: var(--dsw-specific-sidebar-fill, #f9fafb);
        color: var(--dsw-alias-label-primary, #17191c);
        font: 13px system-ui, sans-serif;
      }
      :host([data-theme="dark"]) { background: var(--dsw-specific-sidebar-fill, #1b1b1c); color: var(--dsw-alias-label-primary, #f9fafb); }
      @media (prefers-color-scheme: dark) {
        :host([data-theme="system"]) { background: var(--dsw-specific-sidebar-fill, #1b1b1c); color: var(--dsw-alias-label-primary, #f9fafb); }
      }
      header { display: flex; align-items: center; gap: 10px; height: 100%; padding: 0 12px; app-region: drag; }
      :host([data-platform="windows"]) header { padding-right: 188px; }
      :host([data-platform="macos"]) header { padding-inline: 88px 44px; }
      span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      button { flex: none; padding: 4px 8px; border: 0; border-radius: 6px; background: transparent; color: inherit; font: inherit; cursor: pointer; app-region: no-drag; }
      button:hover { background: color-mix(in srgb, currentColor 8%, transparent); }
      button:active { background: color-mix(in srgb, currentColor 14%, transparent); }
      [role="alert"]:empty { display: none; }
    `);
    shadow.adoptedStyleSheets = [style];
    shadow.innerHTML = `<header><span role="status"></span><button type="button">返回本机</button><span role="alert"></span></header>`;
    const report = (error) => {
      shadow.querySelector('[role="alert"]').textContent = String(error);
    };
    shadow.querySelector('button').addEventListener('click', () => {
      window.__TAURI_INTERNALS__.invoke('return_local').catch(report);
    });
    if (platform === 'macos') {
      shadow.querySelector('header').addEventListener('mousedown', (event) => {
        if (!event.target.closest('button')) {
          window.__WINGMATE_TITLEBAR__?.drag(event);
        }
      });
    }
    if (!['windows', 'macos', 'linux'].includes(platform)) {
      const connection = document.createElement('button');
      connection.type = 'button';
      connection.textContent = '远程连接';
      shadow.querySelector('header').append(connection);
      connection.addEventListener('click', () =>
        window.__TAURI_INTERNALS__.invoke('open_connections').catch(report),
      );
    }
    document.body.append(header);
  }
  header.dataset.theme = theme;
  header.dataset.platform = platform;
  const label = { local: '本机', lan: '局域网', public: '公网', ssh: 'SSH' }[
    transport
  ];
  header.shadowRoot.querySelector('[role="status"]').textContent =
    label + ' · ' + (transport === 'ssh' ? origin : new URL(origin).host);
};
