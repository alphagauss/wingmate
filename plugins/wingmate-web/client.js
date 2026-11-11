import { detailHeading, detailSection, registerPreinstalledItems } from './preinstalled.tsx';

// DSH loads browser plugins as lazy factories and supplies the shared React runtime.
window.__ModuleLoader__.load({
  id: 'dsh-wingmate-web',
  factory(require) {
    const react = require('react');
    const { createElement: h, useEffect, useLayoutEffect, useState, useSyncExternalStore } = react;
    const {
      Button, IconCopyOutlineRegular, IconFolderOpenOutlineRegular, Input, Modal, Switch, Tag,
    } = require('@deepseek-ai/dsh-client-ui-primitives');
    const bridgeEvent = 'wingmate:backend-bridge';
    const getBridge = () => window.__WINGMATE_BACKEND__;
    let remoteHost = null;
    const getRemoteHost = () => remoteHost;
    const recovery = '/_wingmate/recovery';
    const detailStyle = { color: 'var(--dsw-alias-label-secondary)', fontSize: 12, lineHeight: '18px' };
    const alertStyle = { ...detailStyle, color: 'var(--dsw-alias-label-error)' };
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

    function SettingRow({ label, children, control, last, compact }) {
      return h('div', { style: { ...rowStyle, padding: compact ? 0 : rowStyle.padding,
        borderBottom: last ? undefined : '0.5px solid var(--dsw-alias-border-l2)' } },
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

    function ShareSettings({ openSettings }) {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const share = bridge?.share;
      const [status, setStatus] = useState(null);
      const [address, setAddress] = useState('');
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');
      const [copyStatus, setCopyStatus] = useState('');

      useEffect(() => {
        if (!share) return;
        let active = true;
        share({ action: 'status' }).then(value => {
          if (!active) return;
          setStatus(value);
          setAddress(value.suggested_address ?? '');
        }).catch(failure => { if (active) setError(String(failure)); });
        return () => { active = false; };
      }, [share]);

      async function run(command) {
        setPending(true);
        setError('');
        setCopyStatus('');
        try { setStatus(await share(command)); }
        catch (failure) { setError(String(failure)); }
        finally { setPending(false); }
      }
      async function copy() {
        try {
          await navigator.clipboard.writeText(status.pairing_url);
          setCopyStatus('已复制');
        } catch {
          setCopyStatus('请选中链接并手动复制');
        }
      }

      const listening = Boolean(status?.address);
      const fieldStyle = { width: '100%', minWidth: 0, boxSizing: 'border-box' };
      return h(react.Fragment, null,
        share && h('section', { 'aria-label': '局域网分享', style: detailSection },
          h('h2', { style: detailHeading }, '局域网分享'),
          h(SettingRow, {
            label: '允许局域网访问', compact: true, last: true,
            control: h(Switch, {
              label: '允许局域网访问', checked: listening, disabled: pending || !status,
              onChange: enabled => run(enabled ? { action: 'enable', address: address.trim() || null } : { action: 'disable' }),
            }),
          }, h('div', { style: detailStyle }, !status && !error ? '正在读取分享状态…' : '允许已配对的设备访问本机工作台。')),
          h('div', { style: { display: 'grid', gap: 5, width: 'min(100%, 320px)', fontSize: 14 } },
            listening
              ? h(react.Fragment, null, h('span', null, '监听地址'), h('code', { style: valueStyle }, 'http://' + status.address))
              : h(react.Fragment, null,
                h('label', { htmlFor: 'wingmate-share-address' }, '监听地址'),
                h(Input, {
                  id: 'wingmate-share-address', placeholder: '留空自动选择本机 IPv4', style: fieldStyle,
                  value: address, onChange: event => setAddress(event.target.value), disabled: pending || !status,
                }),
              ),
          ),
          listening && h(SettingRow, {
            label: '配对链接', compact: true, last: true,
            control: h(Button, { size: 'sm', variant: 'outline', disabled: pending, onClick: () => run({ action: 'pair' }) }, '生成配对链接'),
          }, h('div', { style: detailStyle }, '链接和二维码可供一台设备使用，有效期为 5 分钟。')),
          status?.pairing_url && h('div', { style: { display: 'grid', gap: 8, width: 'min(100%, 420px)' } },
            status.qr && h('img', {
              src: 'data:image/svg+xml,' + encodeURIComponent(status.qr), width: 144, height: 144,
              alt: '局域网配对二维码', style: { background: 'white', borderRadius: 8 },
            }),
            h('div', { style: { ...actionsStyle, flexWrap: 'nowrap' } },
              h('div', { style: { flex: 1, minWidth: 0, display: 'grid' } }, h(Input, {
                'aria-label': '配对链接', style: fieldStyle, readOnly: true, value: status.pairing_url,
              })),
              h(Button, { size: 'sm', variant: 'outline', icon: h(IconCopyOutlineRegular), disabled: pending, onClick: copy }, '复制'),
            ),
            copyStatus && h('div', { role: 'status', style: detailStyle }, copyStatus),
          ),
          error && h('div', { role: 'alert', style: alertStyle }, error),
        ),
        share && h('section', { 'aria-label': '已配对设备', style: detailSection },
          h('div', { style: { ...actionsStyle, justifyContent: 'space-between' } },
            h('h2', { style: detailHeading }, '已配对设备'),
            h(Button, { size: 'sm', variant: 'ghost', disabled: pending || (!status && !error), onClick: () => run({ action: 'status' }) }, '刷新'),
          ),
          status?.devices.length === 0 && h('span', { style: detailStyle }, '暂无已配对设备'),
          status?.devices.map((device, index) => h(SettingRow, {
            key: device.id, label: device.id.slice(0, 8), last: index === status.devices.length - 1,
            control: h(Button, { size: 'sm', variant: 'outline', disabled: pending, onClick: () => run({ action: 'revoke', id: device.id }) }, '撤销'),
          }, h('div', { style: detailStyle }, new Date(device.paired_at * 1000).toLocaleString()))),
        ),
        openSettings && h('section', { style: detailSection, 'aria-label': '后台服务设置' },
          h('h2', { style: detailHeading }, '后台服务'),
          h(SettingRow, {
            label: '运行与启动', compact: true, last: true,
            control: h(Button, { size: 'sm', variant: 'outline', onClick: openSettings }, '打开设置'),
          }, h('div', { style: detailStyle }, '查看运行状态，管理后台启停和本机命令。')),
        ),
      );
    }

    function BackendSettings() {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const remote = useSyncExternalStore(subscribe, getRemoteHost);
      const canManage = Boolean(bridge || remote?.phase === 'running');
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
          if (bridge) {
            await bridge.manage(action);
          } else {
            const response = await fetch('/_wingmate/manage', {
              method: 'POST', headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ action, expected_instance: remote.instance }),
            });
            if (!response.ok) throw new Error(await response.text());
            location.replace(recovery);
          }
        } catch (failure) {
          setError(String(failure));
          setAction(null);
          setPending(false);
        }
      }

      return h('section', { 'aria-label': '后台服务' },
        h(SettingRow, {
          label: '运行状态',
          control: canManage && h(Tag, { tone: 'success' }, '运行中'),
        },
          h('div', { role: 'status', style: detailStyle },
            bridge ? 'DSH ' + (bridge.dshVersion ?? bridge.version) : remote ? '当前远程主机' : '正在连接 Wingmate 桌面…',
          ),
          (bridge?.runtimeUpdate || bridge?.serviceUpdate) && h('div', { role: 'status', style: { ...actionsStyle, ...detailStyle } },
            h(Tag, { tone: 'warning' }, '待切换'),
            h('span', null, `${[bridge.serviceUpdate && '管理服务', bridge.runtimeUpdate && 'DSH 运行包'].filter(Boolean).join('和')}将在重启后台后更新。`),
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
          bridge.openConnections && h(SettingRow, {
            label: '连接其他主机',
            control: h(Button, {
              size: 'sm', variant: 'outline', disabled: pending,
              onClick: () => bridge.openConnections().catch(failure => setError(String(failure))),
            }, '打开远程连接'),
          }),
        ),
        h(SettingRow, {
          label: '后台管理',
          last: true,
          control: h('div', { style: actionsStyle },
            h(Button, { size: 'sm', variant: 'ghost', disabled: !canManage || pending, onClick: () => setAction('stop') }, '停止后台'),
            h(Button, { size: 'sm', variant: 'outline', disabled: !canManage || pending, onClick: () => setAction('restart') }, '重启后台'),
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
            h(Button, { variant: 'primary', disabled: pending || !canManage, onClick: confirm }, '确认'),
          ),
        }),
      );
    }

    function watchGateway(register) {
      let disposed = false;
      let timer;
      let unregister;
      let runningInstance;
      async function refresh() {
        try {
          if (getBridge()) return;
          const response = await fetch('/_wingmate/status', { signal: AbortSignal.timeout(5000) });
          if (disposed || (!remoteHost && [401, 404].includes(response.status))) return;
          if (response.ok) {
            if (!response.headers.get('Content-Type')?.startsWith('application/json')) return;
            const status = await response.json();
            if (disposed || status.product !== 'wingmate') return;
            const replaced = status.phase === 'running' && runningInstance && runningInstance !== status.instance;
            if (status.phase === 'stopped' || replaced) {
              location.replace(recovery);
              return;
            }
            if (status.phase === 'running') runningInstance = status.instance;
            if (remoteHost?.phase !== status.phase || remoteHost?.instance !== status.instance) {
              remoteHost = status;
              window.dispatchEvent(new Event(bridgeEvent));
            }
            if (register && !unregister) unregister = register();
          }
        } catch { /* Preserve the workbench during temporary connection loss. */ }
        if (!disposed) timer = setTimeout(refresh, 2000);
      }
      refresh();
      return () => {
        disposed = true;
        clearTimeout(timer);
        unregister?.();
      };
    }

    return {
      inject: ['slots'],
      apply(ctx) {
        const desktop = typeof window.__TAURI_INTERNALS__?.invoke === 'function';
        ctx.inject(['remote', 'remote.pluginInventory'], itemContext => {
          registerPreinstalledItems(itemContext, react, { Button, Tag }, desktop, ShareSettings);
        });
        const registerSettings = () => ctx.slots.inject('settings.section', () => ctx.slots.register({
          name: 'settings.section', id: 'wingmate-backend', order: 90, label: '后台服务',
        }, BackendSettings));
        if (desktop) {
          ctx.effect(() => {
            let unregister;
            const registerTitlebar = () => {
              const titlebar = window.__WINGMATE_TITLEBAR__;
              if (!titlebar || unregister) return;
              titlebar.prepare();
              unregister = ctx.slots.inject('shell.overlay', () => ctx.slots.register({
                name: 'shell.overlay', id: 'wingmate-window-titlebar',
              }, function WindowTitlebar() {
                useLayoutEffect(titlebar.mount, []);
                return null;
              }));
            };
            window.addEventListener('wingmate:titlebar', registerTitlebar);
            registerTitlebar();
            return () => {
              window.removeEventListener('wingmate:titlebar', registerTitlebar);
              unregister?.();
            };
          }, 'Wingmate window titlebar');
          registerSettings();
        }
        ctx.effect(() => watchGateway(desktop ? undefined : registerSettings), 'Wingmate gateway status');
      },
    };
  },
});
