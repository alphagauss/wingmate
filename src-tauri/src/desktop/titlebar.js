([address, instance, theme, dev, navigation, macos]) => {
  const home = instance === null;
  if (
    window !== window.top ||
    (home ? location.href : location.origin) !== address
  )
    return;
  if (window.__WINGMATE_TITLEBAR__) {
    window.__WINGMATE_TITLEBAR__.rebind(navigation);
    return;
  }

  const height = '40px';
  const invoke = (action) =>
    window.__TAURI_INTERNALS__.invoke('window_chrome', {
      action,
      instance,
      navigation,
    });
  const controls = document.createElement('div');
  controls.dataset.wingmateTitlebar = '';
  controls.toggleAttribute('data-macos', macos);
  if (home) controls.dataset.homeTheme = theme;
  const shadow = controls.attachShadow({ mode: 'open' });
  const layout = macos ? new CSSStyleSheet() : null;
  layout?.replaceSync(`
    #root {
      box-sizing: border-box;
      padding-top: ${height};
      background: var(--dsw-specific-sidebar-fill, Canvas);
    }
    :root {
      --dsh-frame-chrome-top: ${height};
      --dsh-frame-overlay-top: calc(${height} + 20px);
    }
    :root:has([data-wingmate-remote-header]) {
      --wingmate-local-drag-display: none;
    }
  `);
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
      :host([data-home-theme]:not([data-macos])) {
        background: var(--caption-fill);
      }
      :host([data-home-theme="dark"]) { --caption-fill: #1b1b1c; color: #f9fafb; }
      .drag {
        position: absolute;
        inset: 0 182px 0 0;
        pointer-events: auto;
        app-region: drag;
      }
      :host(:not([data-home-theme]):not([data-macos])) .drag { display: none; }
      :host([data-macos]) .drag {
        inset: 0 44px 0 88px;
        display: var(--wingmate-local-drag-display, block);
      }
      .status {
        position: absolute;
        inset: 0 145px 0 145px;
        align-content: center;
        text-align: center;
        font: 12px system-ui, sans-serif;
        app-region: drag;
      }
      :host([data-macos]) .status { inset-inline: 88px; }
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
      button:hover { background: color-mix(in srgb, currentColor 13%, transparent); }
      button:active { background: color-mix(in srgb, currentColor 26%, transparent); }
      button:focus-visible { outline: 2px solid currentColor; outline-offset: -3px; }
      button[data-action="close"]:hover { background: #e81123; color: white; }
      button[data-action="close"]:active { background: #e8112398; color: white; }
      .connections {
        width: 28px; height: 28px; align-self: center; margin: 0 8px;
        border-radius: var(--dsw-radius-sm, 8px); cursor: pointer;
        color: var(--dsw-alias-label-secondary, inherit);
      }
      .connections:hover { background: var(--dsw-alias-interactive-bg-hover, color-mix(in srgb, currentColor 8%, transparent)); }
      .connections:active { background: var(--dsw-alias-interactive-bg-active, color-mix(in srgb, currentColor 14%, transparent)); }
      .connections:focus-visible {
        outline: var(--dsw-focus-ring-width, 2px) solid var(--dsw-focus-ring-color, var(--dsw-alias-state-business-primary, currentColor));
        outline-offset: -2px;
      }
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
    <div class="buttons"${macos ? '' : ' role="group"'}>
      ${
        macos
          ? ''
          : `
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
      </button>`
      }
    </div>`;
  const buttons = [...shadow.querySelectorAll('button')];
  const maximizeIcon = buttons[1]?.querySelector('path');
  const connection = document.createElement('button');
  connection.type = 'button';
  connection.className = 'connections';
  connection.setAttribute('aria-haspopup', 'dialog');
  connection.ariaLabel = connection.title = '远程连接';
  connection.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4.5" width="18" height="13" rx="2"/><path d="M12 17.5v3m-4 0h8"/></svg>`;
  connection.addEventListener('click', () => {
    const { right, bottom } = connection.getBoundingClientRect();
    window.__TAURI_INTERNALS__
      .invoke('open_connections', {
        anchor: { right: window.innerWidth - right, top: bottom },
      })
      .catch(report);
  });
  shadow.querySelector('.buttons').prepend(connection);
  const status = shadow.querySelector('.status');
  let maximized = false;

  function setDisconnected(disconnected) {
    status.textContent = [
      dev && 'Wingmate · 开发版',
      disconnected && '后台连接暂时中断',
    ]
      .filter(Boolean)
      .join(' · ');
    status.hidden = !status.textContent;
  }
  setDisconnected(false);

  function setMaximized(value) {
    maximized = value;
    if (macos) return;
    const chinese = /^zh\b/i.test(document.documentElement.lang);
    const labels = chinese
      ? ['最小化', value ? '还原' : '最大化', '关闭窗口']
      : ['Minimize', value ? 'Restore' : 'Maximize', 'Close window'];
    shadow.querySelector('.buttons').ariaLabel = chinese
      ? '窗口控件'
      : 'Window controls';
    buttons.forEach((button, i) => {
      button.ariaLabel = labels[i];
      button.title = labels[i];
    });
    maximizeIcon?.setAttribute(
      'd',
      value
        ? 'M2 0h8v8H8v2H0V2h2zm1 1v1h5v5h1V1zM1 3v6h6V3z'
        : 'M0 0h10v10H0zm1 1v8h8V1z',
    );
  }

  const report = (error) => console.error('Wingmate title bar', error);
  function drag(event) {
    if (macos && event.button === 0 && !event.defaultPrevented) {
      invoke(event.detail === 2 ? 'toggleMaximize' : 'drag').catch(report);
    }
  }
  if (macos) shadow.querySelector('.drag').addEventListener('mousedown', drag);
  for (const button of buttons) {
    button.addEventListener('click', () => {
      const { action } = button.dataset;
      invoke(action)
        .then((value) => {
          if (action === 'toggleMaximize') setMaximized(value);
        })
        .catch(report);
    });
  }

  function prepare() {
    const root = document.documentElement;
    if (layout) {
      if (!document.adoptedStyleSheets.includes(layout)) {
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, layout];
      }
    } else {
      root.toggleAttribute('data-windows-titlebar', !home);
      root.style.setProperty('--dsh-windows-titlebar-height', height);
    }
    root.style.setProperty('--wingmate-titlebar-height', height);
  }

  let mounted = false;
  function ready() {
    const requestedNavigation = navigation;
    invoke('ready')
      .then((value) => {
        if (mounted && navigation === requestedNavigation) setMaximized(value);
      })
      .catch((error) => {
        if (!mounted || navigation !== requestedNavigation) return;
        unmount();
        report(error);
      });
  }

  let unmount;
  function mount() {
    if (mounted) return;
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
    presentation.observe(root, {
      attributeFilter: ['lang', 'data-ds-theme-source'],
    });
    syncTheme();
    document.body.append(controls);
    mounted = true;
    if (home) root.dataset.wingmateHomeTitlebar = theme;
    unmount = () => {
      mounted = false;
      presentation.disconnect();
      controls.remove();
      if (layout) {
        document.adoptedStyleSheets = document.adoptedStyleSheets.filter(
          (sheet) => sheet !== layout,
        );
      } else {
        root.removeAttribute('data-windows-titlebar');
        root.style.removeProperty('--dsh-windows-titlebar-height');
      }
      delete root.dataset.wingmateHomeTitlebar;
      root.style.removeProperty('--wingmate-titlebar-height');
    };
    ready();
    return () => {
      if (!mounted) return;
      unmount();
      invoke('fallback').catch(report);
    };
  }

  Object.defineProperty(window, '__WINGMATE_TITLEBAR__', {
    value: Object.freeze({
      prepare,
      mount,
      setMaximized,
      setDisconnected,
      drag,
      rebind(nextNavigation) {
        if (nextNavigation <= navigation) return;
        // Consecutive navigations to the same URL can expose the same document twice.
        navigation = nextNavigation;
        if (mounted) ready();
        else mount();
      },
    }),
  });
  // The DSH preview and sign-in screens can appear before the workbench slots mount.
  const mountApp = () => {
    if (home || document.getElementById('root')) mount();
  };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', mountApp, { once: true });
  } else {
    mountApp();
  }
  window.dispatchEvent(new Event('wingmate:titlebar'));
};
