window.__ModuleLoader__.load({
  id: 'dsh-wingmate-backends',
  factory(require) {
    const { createElement: h } = require('react');
    const agents = window.__WINGMATE_AGENTS__ ?? [];

    function AgentPicker() {
      const url = new URL(location.href);
      const backend = url.searchParams.get('wingmate-agent');
      return h('select', {
        'aria-label': '执行后端',
        value: agents.some(agent => agent.id === backend) ? backend : '',
        onChange(event) {
          const target = event.currentTarget.value;
          if (target) url.searchParams.set('wingmate-agent', target);
          else url.searchParams.delete('wingmate-agent');
          url.hash = '';
          location.assign(url);
        },
      }, h('option', { value: '' }, 'DSH'),
      ...agents.map(agent => h('option', { key: agent.id, value: agent.id }, agent.label)));
    }

    return {
      inject: ['slots'],
      apply(ctx) {
        if (!agents.length) return;
        ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
          name: 'sidebar.footer.action', id: 'wingmate-agent-picker',
        }, AgentPicker));
      },
    };
  },
});
