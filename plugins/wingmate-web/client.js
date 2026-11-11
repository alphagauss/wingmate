import { registerPreinstalledItems } from './preinstalled.tsx';

// DSH loads browser plugins as lazy factories and supplies the shared React runtime.
window.__ModuleLoader__.load({
  id: 'dsh-wingmate-web',
  factory(require) {
    const react = require('react');
    const { createElement: h, useEffect, useLayoutEffect, useState, useSyncExternalStore } = react;
    const {
      Button, IconCopyOutlineRegular, IconFolderOpenOutlineRegular, Modal, Switch, Tag,
    } = require('@deepseek-ai/dsh-client-ui-primitives');
    const bridgeEvent = 'wingmate:backend-bridge';
    const getBridge = () => window.__WINGMATE_BACKEND__;
    const detailStyle = { color: 'var(--dsw-alias-label-secondary)', fontSize: 12, lineHeight: '18px' };
    const alertStyle = { ...detailStyle, color: 'var(--dsw-alias-state-error-primary)' };
    const valueStyle = { display: 'block', ...detailStyle, overflowWrap: 'anywhere' };
    const actionsStyle = { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 };
    const rowStyle = {
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      gap: 24, padding: '16px 0',
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

    function SettingRow({ label, children, control, last }) {
      return h('div', { style: { ...rowStyle, borderBottom: last ? undefined : '0.5px solid var(--dsw-alias-border-l2)' } },
        h('div', { style: { display: 'grid', gap: 4, minWidth: 0, flex: 1 } },
          h('div', { style: { fontSize: 14, lineHeight: '20px' } }, label),
          children,
        ),
        h('div', { style: { flexShrink: 0 } }, control),
      );
    }

    function RegistrationRow({ label, description, configure, pending: backendPending, updateMessage }) {
      const [status, setStatus] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');

      useEffect(() => {
        let active = true;
        configure()
          .then(value => { if (active) setStatus(value); })
          .catch(failure => { if (active) setError(String(failure)); });
        return () => { active = false; };
      }, [configure]);

      async function change(enabled) {
        setPending(true);
        setError('');
        try {
          setStatus(await configure(enabled));
        } catch (failure) {
          setError(String(failure));
          try { setStatus(await configure()); } catch { setStatus(null); }
        } finally {
          setPending(false);
        }
      }

      const disabled = backendPending || pending || !status;
      return h(SettingRow, {
        label,
        control: h(Switch, { label, checked: status?.enabled ?? false, disabled, onChange: change }),
      },
        h('div', { style: detailStyle }, description),
        status?.needsUpdate && h('div', { style: { ...actionsStyle, ...detailStyle } },
          updateMessage,
          h(Button, { size: 'sm', variant: 'outline', disabled, onClick: () => change(true) }, '重新注册'),
        ),
        status?.registered && !status.enabled && h('div', { style: detailStyle }, '此启动项当前未启用。'),
        error && h('div', { role: 'alert', style: alertStyle }, error),
      );
    }

    function BackendSettings() {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const [action, setAction] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');
      const [copyStatus, setCopyStatus] = useState('');

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

      return h('section', { 'aria-label': '后台服务' },
        h(SettingRow, {
          label: '运行状态',
          control: bridge && h(Tag, { tone: 'success' }, '运行中'),
        },
          h('div', { role: 'status', style: detailStyle },
            bridge ? `DSH ${bridge.dshVersion ?? bridge.version}` : '正在连接 Wingmate 桌面…',
          ),
          bridge?.runtimeUpdate && h('div', { role: 'status', style: { ...actionsStyle, ...detailStyle } },
            h(Tag, { tone: 'warning' }, '待切换'),
            h('span', null, '新运行包将在重启后台后启用。'),
          ),
        ),
        bridge && h('div', null,
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
          bridge.loginStartup && h(RegistrationRow, {
            label: '登录系统后启动后台',
            description: '登录后在后台运行，不打开工作台。',
            configure: bridge.loginStartup,
            pending,
            updateMessage: '启动项指向的程序路径已变化。',
          }),
          bridge.dshCommand && h(RegistrationRow, {
            label: '启用终端命令',
            description: '在新开的终端中使用 wingmate 管理后台，或用 dsh 访问当前后台的运行时和数据目录。',
            configure: bridge.dshCommand,
            pending,
            updateMessage: '命令注册需要更新。',
          }),
        ),
        h(SettingRow, {
          label: '后台管理',
          last: true,
          control: h('div', { style: actionsStyle },
            h(Button, { size: 'sm', variant: 'ghost', disabled: !bridge || pending, onClick: () => setAction('stop') }, '停止后台'),
            h(Button, { size: 'sm', variant: 'outline', disabled: !bridge || pending, onClick: () => setAction('restart') }, '重启后台'),
          ),
        },
          h('div', { style: detailStyle }, '关闭窗口后，后台和正在运行的任务会继续。'),
          error && h('div', { role: 'alert', style: alertStyle }, error),
        ),
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
        const desktop = typeof window.__TAURI_INTERNALS__?.invoke === 'function';
        ctx.inject(['remote', 'remote.pluginInventory'], itemContext => {
          registerPreinstalledItems(itemContext, react, { Button, Tag }, desktop);
        });
        if (!desktop) return;
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
