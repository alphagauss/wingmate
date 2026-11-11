window.__ModuleLoader__.load({
  id: 'dsh-wingmate-backends',
  factory(require) {
    const { createElement: h, useSyncExternalStore } = require('react');
    const agents = window.__WINGMATE_AGENTS__ ?? [];
    const unavailable = { getSnapshot: () => false, subscribe: () => () => {} };

    function AgentPicker() {
      const error = useSyncExternalStore(listener => {
        window.addEventListener('wingmate:agent-status', listener);
        return () => window.removeEventListener('wingmate:agent-status', listener);
      }, () => window.__WINGMATE_AGENT_ERROR__ ?? '');
      const url = new URL(location.href);
      const backend = url.searchParams.get('wingmate-agent');
      return h('div', null, h('select', {
        'aria-label': '执行后端',
        style: { font: 'inherit', color: 'inherit', background: 'transparent', borderRadius: 6, padding: '4px 8px' },
        value: agents.some(agent => agent.id === backend) ? backend : '',
        onChange(event) {
          const target = event.currentTarget.value;
          if (target) url.searchParams.set('wingmate-agent', target);
          else url.searchParams.delete('wingmate-agent');
          url.hash = '';
          location.assign(url);
        },
      }, h('option', { value: '' }, 'DSH'),
      ...agents.map(agent => h('option', { key: agent.id, value: agent.id }, agent.label))),
      error && h('div', { role: 'alert', style: { maxWidth: 220, whiteSpace: 'normal', fontSize: 12 } }, error));
    }

    return {
      inject: ['slots', 'remoteAccess'],
      apply(ctx) {
        if (!agents.length) return;
        const backend = new URL(location.href).searchParams.get('wingmate-agent');
        if (backend) {
          const access = ctx.remoteAccess;
          const source = access.source;
          ctx.effect(() => {
            access.source = function (kind, id, action) {
              if (kind === 'session' && id.startsWith(backend + ':')
                && (action.startsWith('messageFeedback/') || action === 'session/fork')) return unavailable;
              return source.call(this, kind, id, action);
            };
            return () => { access.source = source; };
          }, 'wingmate-backends: session capabilities');
        }
        ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
          name: 'sidebar.footer.action', id: 'wingmate-agent-picker',
        }, AgentPicker));
      },
    };
  },
});
