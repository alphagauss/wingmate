([address, instance, theme, dev, navigation, connectionPanel]) => {
  const home = instance === null;
  if (window !== window.top || (home ? location.href : location.origin) !== address
    || window.__WINGMATE_TITLEBAR__) return;

  const height = '40px';
  const invoke = action => window.__TAURI_INTERNALS__.invoke('window_chrome', { action, instance, navigation });
  const controls = document.createElement('div');
  controls.dataset.wingmateTitlebar = '';
  if (home) controls.dataset.homeTheme = theme;
  const shadow = controls.attachShadow({ mode: 'open' });
  const stylesheet = new CSSStyleSheet();
  stylesheet.replaceSync(`
      :host {
        position: fixed;
        inset: 0 0 auto;
        z-index: 1100;
        display: flex;
        justify-content: flex-end;
        height: ${height};
        pointer-events: none;
        color: var(--dsw-alias-label-primary, #0f1115);
        --caption-fill: var(--dsw-specific-sidebar-fill, #f9fafb);
      }
      :host([data-home-theme]) {
        background: var(--caption-fill);
      }
      :host([data-home-theme="dark"]) { --caption-fill: #1b1b1c; color: #f9fafb; }
      .drag {
        position: absolute;
        inset: 0 204px 0 0;
        pointer-events: auto;
        app-region: drag;
      }
      :host(:not([data-home-theme])) .drag { display: none; }
      .status {
        position: absolute;
        inset: 0 145px 0 145px;
        align-content: center;
        text-align: center;
        font: 12px system-ui, sans-serif;
        app-region: drag;
      }
      .buttons {
        display: flex;
        pointer-events: auto;
        app-region: no-drag;
      }
      button {
        display: grid;
        place-items: center;
        width: 46px;
        height: 100%;
        border: 0;
        border-radius: 0;
        padding: 0;
        background: transparent;
        color: inherit;
        transition: background-color var(--ds-transition-duration-fast, .1s)
          var(--ds-ease-in-out, cubic-bezier(.4, 0, .2, 1)),
          color var(--ds-transition-duration-fast, .1s)
          var(--ds-ease-in-out, cubic-bezier(.4, 0, .2, 1));
      }
      button svg { pointer-events: none; }
      .connections { width: 66px; font: 12px system-ui, sans-serif; }
      button:hover { background: color-mix(in srgb, currentColor 13%, transparent); }
      button:active { background: color-mix(in srgb, currentColor 26%, transparent); }
      button:focus-visible { outline: 2px solid currentColor; outline-offset: -3px; }
      button[data-action="close"]:hover { background: #e81123; color: white; }
      button[data-action="close"]:active { background: #e8112398; color: white; }
      @media (prefers-color-scheme: dark) {
        :host([data-home-theme="system"]) { --caption-fill: #1b1b1c; color: #f9fafb; }
      }
      @media (prefers-reduced-motion: reduce) {
        button { transition: none; }
      }
  `);
  shadow.adoptedStyleSheets = [stylesheet];
  shadow.innerHTML = `
    <div class="drag"></div>
    <span class="status" role="status" hidden></span>
    <div class="buttons" role="group">
      <button type="button" data-action="minimize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path fill="currentColor" d="M0 5h10v1H0z"/>
        </svg>
      </button>
      <button type="button" data-action="toggleMaximize">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path fill="currentColor" fill-rule="evenodd"/>
        </svg>
      </button>
      <button type="button" data-action="close">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path stroke="currentColor" d="m.5.5 9 9m0-9-9 9"/>
        </svg>
      </button>
    </div>`;
  const buttons = [...shadow.querySelectorAll('button')];
  const maximizeIcon = buttons[1].querySelector('path');
  if (!connectionPanel) {
    const connection = document.createElement('button');
    connection.type = 'button';
    connection.className = 'connections';
    connection.textContent = '连接';
    connection.title = '连接其他 Wingmate 主机';
    connection.addEventListener('click', () => window.__TAURI_INTERNALS__.invoke('open_connections').catch(report));
    shadow.querySelector('.buttons').prepend(connection);
  }
  const status = shadow.querySelector('.status');
  let maximized = false;

  function setDisconnected(disconnected) {
    status.textContent = [
      dev && 'Wingmate · 开发版',
      disconnected && '后台连接暂时中断',
    ].filter(Boolean).join(' · ');
    status.hidden = !status.textContent;
  }
  setDisconnected(false);

  function setMaximized(value) {
    maximized = value;
    const chinese = /^zh\b/i.test(document.documentElement.lang);
    const labels = chinese
      ? ['最小化', value ? '还原' : '最大化', '关闭窗口']
      : ['Minimize', value ? 'Restore' : 'Maximize', 'Close window'];
    shadow.querySelector('.buttons').ariaLabel = chinese ? '窗口控件' : 'Window controls';
    buttons.forEach((button, i) => {
      button.ariaLabel = labels[i];
      button.title = labels[i];
    });
    maximizeIcon.setAttribute('d', value
      ? 'M2 0h8v8H8v2H0V2h2zm1 1v1h5v5h1V1zM1 3v6h6V3z'
      : 'M0 0h10v10H0zm1 1v8h8V1z');
  }

  const report = error => console.error('Wingmate title bar', error);
  for (const button of buttons) {
    button.addEventListener('click', () => {
      const { action } = button.dataset;
      invoke(action).then(value => {
        if (action === 'toggleMaximize') setMaximized(value);
      }).catch(report);
    });
  }

  function prepare() {
    const root = document.documentElement;
    root.toggleAttribute('data-windows-titlebar', !home);
    root.style.setProperty('--dsh-windows-titlebar-height', height);
  }

  function mount() {
    prepare();
    setMaximized(maximized);
    const root = document.documentElement;
    let sentTheme;
    const syncTheme = () => {
      const source = root.getAttribute('data-ds-theme-source');
      if (!home && source && source !== sentTheme) {
        sentTheme = source;
        invoke({ theme: source }).catch(report);
      }
    };
    const presentation = new MutationObserver(() => {
      setMaximized(maximized);
      syncTheme();
    });
    presentation.observe(root, { attributeFilter: ['lang', 'data-ds-theme-source'] });
    syncTheme();
    document.body.append(controls);
    let mounted = true;
    if (home) root.dataset.wingmateHomeTitlebar = theme;
    const remove = () => {
      mounted = false;
      presentation.disconnect();
      controls.remove();
      delete root.dataset.wingmateHomeTitlebar;
      root.removeAttribute('data-windows-titlebar');
      root.style.removeProperty('--dsh-windows-titlebar-height');
    };
    invoke('ready').then(value => {
      if (mounted) setMaximized(value);
    }).catch(error => {
      if (!mounted) return;
      remove();
      report(error);
    });
    return () => {
      if (!mounted) return;
      remove();
      invoke('fallback').catch(report);
    };
  }

  Object.defineProperty(window, '__WINGMATE_TITLEBAR__', {
    value: Object.freeze({
      prepare, mount, setMaximized, setDisconnected,
    }),
  });
  if (home) {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', mount, { once: true });
    } else {
      mount();
    }
  }
  window.dispatchEvent(new Event('wingmate:titlebar'));
}
