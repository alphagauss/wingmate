// DSH loads browser plugins as lazy factories and supplies the shared React runtime.
window.__ModuleLoader__.load({
  id: 'dsh-wingmate-web',
  factory(require) {
    const { createElement: h, useEffect, useLayoutEffect, useState, useSyncExternalStore } = require('react');
    const {
      Button, IconCopyOutlineRegular, IconFolderOpenOutlineRegular, Modal, Switch, Tag,
    } = require('@deepseek-ai/dsh-client-ui-primitives');
    const bridgeEvent = 'wingmate:backend-bridge';
    const getBridge = () => window.__WINGMATE_BACKEND__;
    const startupLabel = '登录系统后启动后台';
    const detailStyle = { color: 'var(--dsw-alias-label-secondary)', fontSize: 12, lineHeight: '18px' };
    const alertStyle = { ...detailStyle, color: 'var(--dsw-alias-state-error-primary)' };
    const valueStyle = { display: 'block', ...detailStyle, overflowWrap: 'anywhere' };
    const actionsStyle = { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 };
    const groupStyle = {
      display: 'grid', gap: 1, overflow: 'clip', borderRadius: 'var(--dsw-radius-md)',
      border: '1px solid var(--dsw-alias-border-l2)', background: 'var(--dsw-alias-border-l2)',
    };
    const rowStyle = {
      display: 'grid', gridTemplateColumns: 'minmax(0, 1fr) auto', alignItems: 'center',
      gap: 12, padding: '12px 14px', background: 'var(--dsw-alias-bg-layer-2)',
    };

    function subscribe(update) {
      window.addEventListener(bridgeEvent, update);
      return () => window.removeEventListener(bridgeEvent, update);
    }

    function WindowTitlebar() {
      useLayoutEffect(() => {
        let unmount;
        const mount = () => { unmount ??= window.__WINGMATE_TITLEBAR__?.mount(); };
        window.addEventListener('wingmate:titlebar', mount);
        mount();
        return () => {
          window.removeEventListener('wingmate:titlebar', mount);
          unmount?.();
        };
      }, []);
      return null;
    }

    function SettingRow({ label, children, control }) {
      return h('div', { style: rowStyle },
        h('div', { style: { display: 'grid', gap: 4, minWidth: 0 } },
          h('div', { style: { fontSize: 14, fontWeight: 500, lineHeight: '20px' } }, label),
          children,
        ),
        control,
      );
    }

    function BackendSettings() {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const [action, setAction] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');
      const [copyStatus, setCopyStatus] = useState('');
      const [startup, setStartup] = useState(null);
      const [startupPending, setStartupPending] = useState(false);
      const [startupError, setStartupError] = useState('');

      useEffect(() => {
        if (!bridge?.loginStartup) return;
        let active = true;
        bridge.loginStartup()
          .then(value => { if (active) setStartup(value); })
          .catch(failure => { if (active) setStartupError(String(failure)); });
        return () => { active = false; };
      }, [bridge]);

      async function setLoginStartup(enabled) {
        setStartupPending(true);
        setStartupError('');
        try {
          setStartup(await bridge.loginStartup(enabled));
        } catch (failure) {
          setStartupError(String(failure));
          try { setStartup(await bridge.loginStartup()); } catch { setStartup(null); }
        } finally {
          setStartupPending(false);
        }
      }

      async function openDshHome() {
        setError('');
        try {
          await bridge.openDshHome();
        } catch (failure) {
          setError(String(failure));
        }
      }

      async function copyWebLink() {
        setCopyStatus('');
        try {
          await bridge.copyWebLink();
          setCopyStatus('已复制');
        } catch {
          setCopyStatus('复制失败，请重试。');
        }
      }

      async function confirm() {
        setPending(true);
        setError('');
        try {
          await bridge.manage(action);
        } catch (failure) {
          setError(String(failure));
          setAction(null);
          setPending(false);
        }
      }

      return h('section', { 'aria-label': '后台服务', style: { display: 'grid', gap: 16, minWidth: 0 } },
        h('header', { style: { display: 'grid', gap: 8 } },
          h('div', { style: { ...actionsStyle, justifyContent: 'space-between' } },
            h('h2', { style: { margin: 0, fontSize: 20, lineHeight: '28px' } }, '后台服务'),
            h('div', { role: 'status', style: { ...actionsStyle, ...detailStyle } },
              bridge
                ? [h(Tag, { key: 'state', tone: 'success' }, '运行中'),
                  h('span', { key: 'version' }, `DSH ${bridge.dshVersion ?? bridge.version}`)]
                : '正在连接 Wingmate 桌面…',
            ),
          ),
          bridge?.runtimeUpdate && h('div', { style: { ...actionsStyle, ...detailStyle } },
            h(Tag, { tone: 'warning' }, '待切换'),
            h('span', null, '新运行包将在重启后台后启用。'),
          ),
        ),
        bridge && h('div', { style: groupStyle },
          bridge.copyWebLink && h(SettingRow, {
            label: 'Web 地址',
            control: h(Button, { size: 'sm', variant: 'outline', icon: h(IconCopyOutlineRegular), disabled: pending, onClick: copyWebLink }, '复制链接'),
          },
            h('code', { style: valueStyle }, window.location.origin),
            copyStatus && h('span', { role: 'status', style: detailStyle }, copyStatus),
          ),
          bridge.openDshHome && h(SettingRow, {
            label: 'DSH 数据目录',
            control: h(Button, { size: 'sm', variant: 'outline', icon: h(IconFolderOpenOutlineRegular), disabled: pending, onClick: openDshHome }, '打开文件夹'),
          },
            h('code', { style: valueStyle }, bridge.dshHome),
          ),
          bridge.loginStartup && h(SettingRow, {
            label: startupLabel,
            control: h(Switch, {
              label: startupLabel,
              checked: startup?.enabled ?? false,
              disabled: pending || startupPending || !startup,
              onChange: setLoginStartup,
            }),
          },
            h('div', { style: detailStyle }, '登录后在后台运行，不打开工作台。'),
            startup?.needsUpdate && h('div', { style: { ...actionsStyle, ...detailStyle } },
              '启动项指向的程序路径已变化。',
              h(Button, { size: 'sm', variant: 'outline', disabled: pending || startupPending, onClick: () => setLoginStartup(true) }, '重新注册'),
            ),
            startup?.registered && !startup.enabled && h('div', { style: detailStyle }, '此启动项当前未启用。'),
            startupError && h('div', { role: 'alert', style: alertStyle }, startupError),
          ),
        ),
        h('div', { style: { ...actionsStyle, justifyContent: 'space-between' } },
          h('span', { style: detailStyle }, '关闭窗口后，后台和正在运行的任务会继续。'),
          h('div', { style: actionsStyle },
            h(Button, { size: 'sm', variant: 'ghost', disabled: !bridge || pending, onClick: () => setAction('stop') }, '停止后台'),
            h(Button, { size: 'sm', variant: 'outline', disabled: !bridge || pending, onClick: () => setAction('restart') }, '重启后台'),
          ),
        ),
        error && h('div', { role: 'alert', style: alertStyle }, error),
        h(Modal, {
          open: action !== null,
          onClose: () => { if (!pending) setAction(null); },
          title: action === 'stop' ? '停止后台？' : '重启后台？',
          description: '这会中断正在运行的任务。',
          closeLabel: '关闭确认',
          footer: h('div', { style: { display: 'flex', gap: 8 } },
            h(Button, { 'data-modal-autofocus': true, disabled: pending, onClick: () => setAction(null) }, '取消'),
            h(Button, { variant: 'primary', disabled: pending || !bridge, onClick: confirm }, '确认'),
          ),
        }),
      );
    }

    return {
      inject: ['slots'],
      apply(ctx) {
        if (typeof window.__TAURI_INTERNALS__?.invoke !== 'function') return;
        window.__WINGMATE_TITLEBAR__?.prepare();
        ctx.slots.inject('shell.overlay', () => ctx.slots.register({
          name: 'shell.overlay', id: 'wingmate-window-titlebar',
        }, WindowTitlebar));
        ctx.slots.inject('settings.section', () => ctx.slots.register({
          name: 'settings.section', id: 'wingmate-backend', order: 90, label: '后台服务',
        }, BackendSettings));
      },
    };
  },
});
