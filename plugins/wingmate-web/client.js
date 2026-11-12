import { detailHeading, detailSection, registerPreinstalledItems } from './preinstalled.tsx';

// DSH loads browser plugins as lazy factories and supplies the shared React runtime.
window.__ModuleLoader__.load({
  id: 'dsh-wingmate-web',
  factory(require) {
    const react = require('react');
    const { createElement: h, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } = react;
    const {
      Button, IconCopyOutlineRegular, IconFolderOpenOutlineRegular, Modal, Switch, Tag,
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

    function NamedTunnelSettings({ tunnel, pending, save }) {
      const [notice, setNotice] = useState('');
      const field = { display: 'grid', gap: 4 };
      const input = { padding: '6px 8px', borderRadius: 6, border: '1px solid var(--dsw-alias-border-l2)' };
      async function submit(event) {
        event.preventDefault();
        const form = event.currentTarget;
        const data = new FormData(form);
        const token = data.get('token')?.trim() || undefined;
        setNotice('');
        if (await save({ action: 'tunnel_configure', hostname: data.get('hostname'), token })) {
          form.elements.namedItem('token').value = '';
          setNotice('已保存，下次开启或重启隧道时生效。');
        }
      }
      return h('section', { 'aria-label': '具名隧道', style: detailSection },
        h('h2', { style: detailHeading }, '具名隧道'),
        h('div', { style: { ...actionsStyle, ...detailStyle } },
          '在 Cloudflare 发布路由中填写 Service URL',
          h('code', { style: valueStyle }, tunnel.named_service),
          h(Button, { size: 'sm', variant: 'outline', onClick: async () => {
            try {
              await navigator.clipboard.writeText(tunnel.named_service);
              setNotice('已复制回源地址。');
            } catch {
              setNotice('请手动复制回源地址。');
            }
          } }, '复制地址'),
        ),
        h('form', { method: 'post', style: { ...detailSection, maxWidth: 420 }, onSubmit: submit },
          h('label', { style: field }, '公开域名',
            h('input', { name: 'hostname', style: input, defaultValue: tunnel.hostname ?? '', required: true,
              placeholder: 'wingmate.example.com', spellCheck: false, autoComplete: 'off', disabled: pending }),
          ),
          h('label', { style: field }, 'Tunnel Token',
            h('input', { name: 'token', type: 'password', style: input, autoComplete: 'off', disabled: pending,
              required: !tunnel.hostname, placeholder: tunnel.hostname ? '留空保留已保存的 Token' : '粘贴 Cloudflare Tunnel Token' }),
          ),
          h('div', { style: actionsStyle },
            h(Button, { type: 'submit', size: 'sm', variant: 'outline', disabled: pending }, '保存'),
            h('span', { role: 'status', style: detailStyle }, notice),
          ),
        ),
      );
    }

    function ShareSettings({ openSettings }) {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const share = bridge?.share;
      const [status, setStatus] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');

      useEffect(() => {
        if (!share) return;
        let active = true;
        share({ action: 'status' })
          .then(value => { if (active) setStatus(value); })
          .catch(failure => { if (active) setError(String(failure)); });
        return () => { active = false; };
      }, [share]);

      async function run(command) {
        setPending(true);
        setError('');
        try {
          setStatus(await share(command));
          return true;
        } catch (failure) {
          setError(String(failure));
          return false;
        } finally {
          setPending(false);
        }
      }

      const listening = Boolean(status?.address);
      const addresses = status?.lan_addresses?.length ? status.lan_addresses : [status?.address];
      return h(react.Fragment, null,
        share && h('section', { 'aria-label': '局域网分享', style: detailSection },
          h('h2', { style: detailHeading }, '局域网分享'),
          h(SettingRow, {
            label: '允许局域网访问', compact: true, last: true,
            control: h(Switch, {
              label: '允许局域网访问', checked: listening, disabled: pending || !status,
              onChange: enabled => run(enabled ? { action: 'enable', address: null } : { action: 'disable' }),
            }),
          },
            !status && !error && h('div', { style: detailStyle }, '正在读取分享状态…'),
            status && !listening && h('div', { style: detailStyle },
              '开启后，已配对设备可通过本机的局域网地址连接。',
            ),
            listening && h('div', { style: { ...actionsStyle, ...detailStyle } },
              '监听地址', addresses.map(address => h('code', { key: address, style: valueStyle }, 'http://' + address)),
            ),
          ),
        ),
        share && status?.tunnel && h(NamedTunnelSettings, { tunnel: status.tunnel, pending, save: run }),
        share && error && h('div', { role: 'alert', style: alertStyle }, error),
        share && h('section', { 'aria-label': '已配对设备', style: detailSection },
          h('div', { style: { ...actionsStyle, justifyContent: 'space-between' } },
            h('h2', { style: detailHeading }, '已配对设备'),
            h(Button, { size: 'sm', variant: 'ghost', disabled: pending || (!status && !error), onClick: () => run({ action: 'status' }) }, '刷新'),
          ),
          status?.devices.length === 0 && h('span', { style: detailStyle }, '暂无已配对设备'),
          status?.devices.map((device, index) => h(SettingRow, {
            key: device.id, label: device.id.slice(0, 8), last: index === status.devices.length - 1,
            control: h(Button, { size: 'sm', variant: 'outline', disabled: pending, onClick: () => run({ action: 'revoke', id: device.id }) }, '撤销'),
          }, h('div', { style: detailStyle },
            `${{ local: '本机', lan: '局域网', public: '公网' }[device.access]} · ${new Date(device.paired_at * 1000).toLocaleString()}`,
          ))),
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
      const canRestart = canManage && (!bridge || bridge.restartDsh);
      const [action, setAction] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');
      const [copyStatus, setCopyStatus] = useState('');
      const [localAddress, setLocalAddress] = useState(undefined);
      const connectionText = useRef(null);
      const dshOrigin = bridge ? window.location.origin : remote?.dsh_origin;
      const connectionAddress = bridge ? localAddress : remote && window.location.origin;
      const updating = action?.kind === 'update';
      const stopping = action?.kind === 'stop';
      const canConfirm = updating ? bridge?.update : stopping ? canManage : canRestart;
      const updates = [bridge?.serviceUpdate && 'Wingmate 管理服务', bridge?.runtimeUpdate && 'DSH 运行包']
        .filter(Boolean).join('和');

      useEffect(() => {
        if (!bridge) return;
        let active = true;
        bridge.share({ action: 'status' })
          .then(status => {
            if (active) setLocalAddress(status.local_address ? 'http://' + status.local_address : null);
          })
          .catch(() => { if (active) setLocalAddress(null); });
        return () => { active = false; };
      }, [bridge]);

      async function openDshHome() {
        setError('');
        try {
          await bridge.openDshHome();
        } catch (failure) {
          setError(String(failure));
        }
      }

      async function copyConnectionAddress() {
        if (navigator.clipboard) {
          try {
            await navigator.clipboard.writeText(connectionAddress);
            setCopyStatus('已复制');
            return;
          } catch { /* Fall back to selecting the address. */ }
        }
        const selection = window.getSelection();
        if (!selection || !connectionText.current) return;
        const range = document.createRange();
        range.selectNodeContents(connectionText.current);
        selection.removeAllRanges();
        selection.addRange(range);
        try {
          if (document.execCommand('copy')) {
            selection.removeAllRanges();
            setCopyStatus('已复制');
            return;
          }
        } catch { /* Keep the selected address available for manual copying. */ }
        setCopyStatus('请手动复制已选中的地址');
      }

      function choose(kind) {
        setAction({ kind, instance: remote?.instance });
      }

      async function confirm() {
        setPending(true);
        setError('');
        try {
          if (bridge) {
            if (updating) await bridge.update();
            else if (stopping) await bridge.manage('stop');
            else await bridge.restartDsh();
          } else {
            const response = await fetch('/_wingmate/manage', {
              method: 'POST', headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ action: action.kind, expected_instance: action.instance }),
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
            bridge ? 'DSH ' + (bridge.dshVersion ?? bridge.version) : remote?.phase === 'unavailable'
              ? '连接已失效，请从主机获取新的配对链接。' : remote ? '当前主机' : '正在连接 Wingmate 桌面…',
          ),
          updates && h('div', { role: 'status', style: { ...actionsStyle, ...detailStyle } },
            h(Tag, { tone: 'warning' }, '待更新'),
            h('span', null, updates),
          ),
        ),
        dshOrigin && h(SettingRow, { label: 'DSH 本机地址' },
          h('code', { style: valueStyle }, dshOrigin),
        ),
        (bridge || remote) && h(SettingRow, {
          label: 'Wingmate 连接地址',
          control: connectionAddress && h(Button, {
            size: 'sm', variant: 'outline', icon: h(IconCopyOutlineRegular), onClick: copyConnectionAddress,
          }, '复制地址'),
        },
          connectionAddress
            ? h('code', { ref: connectionText, style: valueStyle }, connectionAddress)
            : h('div', { style: detailStyle }, localAddress === undefined ? '正在读取本机地址…' : '本机 Web 入口不可用'),
          copyStatus && h('span', { role: 'status', style: detailStyle }, copyStatus),
        ),
        bridge && h('div', null,
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
            h(Button, { size: 'sm', variant: 'ghost', disabled: !canManage || pending, onClick: () => choose('stop') }, '停止 DSH'),
            h(Button, { size: 'sm', variant: 'outline', disabled: !canRestart || pending, onClick: () => choose('restart') }, '重启 DSH'),
            bridge?.update && h(Button, {
              size: 'sm', variant: 'outline', disabled: pending, onClick: () => choose('update'),
            }, '重启并更新'),
          ),
        },
          h('div', { style: detailStyle }, '关闭窗口后，后台和正在运行的任务会继续。'),
          error && h('div', { role: 'alert', style: alertStyle }, error),
        ),
        h(Modal, {
          open: action !== null,
          onClose: () => { if (!pending) setAction(null); },
          title: updating ? '重启并更新？' : stopping ? '停止 DSH？' : '重启 DSH？',
          description: updating
            ? `将更新${updates}。这会中断正在运行的任务。${bridge.serviceUpdate ? '已连接的客户端会短暂断开。' : ''}`
            : '这会中断正在运行的任务。',
          closeLabel: '关闭确认',
          footer: h('div', { style: { display: 'flex', gap: 8 } },
            h(Button, { 'data-modal-autofocus': true, disabled: pending, onClick: () => setAction(null) }, '取消'),
            h(Button, { variant: 'primary', disabled: pending || !canConfirm, onClick: confirm }, '确认'),
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
          if (response.status === 401 && remoteHost?.phase !== 'unavailable') {
            remoteHost = { phase: 'unavailable' };
            window.dispatchEvent(new Event(bridgeEvent));
          }
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
          ctx.inject(['pluginNavigation'], scope => {
            scope.effect(() => {
              const open = () => {
                if (!getBridge()) return;
                scope.pluginNavigation.openList();
                const entry = scope.slots.entries('main').find(item => item.options.key === 'plugins');
                entry?.store?.create().actions.setView({ kind: 'item', id: 'dsh-wingmate-web' });
              };
              window.addEventListener('wingmate:tunnel-settings', open);
              return () => window.removeEventListener('wingmate:tunnel-settings', open);
            }, 'Wingmate tunnel settings navigation');
          });
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
