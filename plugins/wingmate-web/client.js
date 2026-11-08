// DSH loads browser plugins as lazy factories and supplies the shared React runtime.
window.__ModuleLoader__.load({
  id: 'dsh-wingmate-web',
  factory(require) {
    const { createElement: h, useState, useSyncExternalStore } = require('react');
    const { Button, Modal } = require('@deepseek-ai/dsh-client-ui-primitives');
    const bridgeEvent = 'wingmate:backend-bridge';
    const getBridge = () => window.__WINGMATE_BACKEND__;

    function subscribe(update) {
      window.addEventListener(bridgeEvent, update);
      return () => window.removeEventListener(bridgeEvent, update);
    }

    function BackendSettings() {
      const bridge = useSyncExternalStore(subscribe, getBridge);
      const [action, setAction] = useState(null);
      const [pending, setPending] = useState(false);
      const [error, setError] = useState('');

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
        h('h2', null, '后台服务'),
        h('p', { role: 'status' }, bridge ? `运行中 · DSH ${bridge.version}` : '正在连接 Wingmate 桌面…'),
        h('p', null, '关闭窗口后，后台和正在运行的任务会继续。'),
        h('div', { style: { display: 'flex', gap: 8 } },
          h(Button, { variant: 'outline', disabled: !bridge || pending, onClick: () => setAction('stop') }, '停止后台'),
          h(Button, { variant: 'outline', disabled: !bridge || pending, onClick: () => setAction('restart') }, '重启后台'),
        ),
        error && h('p', { role: 'alert' }, error),
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
        ctx.slots.inject('settings.section', () => ctx.slots.register({
          name: 'settings.section', id: 'wingmate-backend', order: 90, label: '后台服务',
        }, BackendSettings));
      },
    };
  },
});
