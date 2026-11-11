import type { ComponentType, ReactNode } from 'react';
import { diagnosticsPath } from './diagnostics.ts';
import type { BackendStatus } from './diagnostics.ts';

type Agent = { id: string; label: string };
type Button = ComponentType<{
  size: 'sm'; variant: 'outline' | 'ghost'; disabled?: boolean; onClick(): void; children: ReactNode;
}>;
type Observable = { getSnapshot(): boolean; subscribe(listener: () => void): () => void };
type ClientModule = { inject: string[]; apply(ctx: ClientContext): void };
type ClientContext = {
  remoteAccess: { source(kind: string, id: string, action: string): Observable };
  effect(install: () => () => void, label: string): void;
  get(name: 'pluginNavigation'): { openList(): void } | undefined;
  slots: {
    inject(name: string, register: () => () => void): void;
    register(options: { name: string; id?: string }, component: ComponentType): () => void;
    entries(name: string): readonly {
      options: { key?: string; id?: string };
      store?: { create(): { actions: { setTab(id: string): void } } };
    }[];
  };
};

declare global {
  interface Window {
    __WINGMATE_AGENTS__?: Agent[];
    __WINGMATE_AGENT_ERROR__?: string;
    __ModuleLoader__: { load(entry: { id: string; factory(require: (name: string) => unknown): ClientModule }): void };
  }
}

type DiagnosticsSnapshot = { rows?: BackendStatus[]; checking?: string; readFailed?: boolean };

function createDiagnosticsSource() {
  let snapshot: DiagnosticsSnapshot = {};
  let revision = 0;
  const listeners = new Set<() => void>();
  function publish(next: DiagnosticsSnapshot) {
    snapshot = next;
    for (const listener of listeners) listener();
  }
  async function request(id?: string) {
    if (snapshot.checking) return;
    const current = ++revision;
    if (id) publish({ rows: snapshot.rows, checking: id });
    try {
      const response = await fetch(diagnosticsPath, id ? {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }),
      } : undefined);
      if (!response.ok) throw new Error('Failed to read backend diagnostics');
      const rows: BackendStatus[] = await response.json();
      if (!Array.isArray(rows)) throw new Error('Invalid backend diagnostics');
      if (current === revision) publish({ rows });
    } catch {
      if (current === revision) publish({ rows: snapshot.rows, readFailed: true });
    }
  }
  return {
    request,
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  };
}

const connectionLabels = {
  unchecked: '尚未检查连接', connected: '最近一次连接成功', failed: '最近一次连接失败', disconnected: '连接已断开',
};
const agentStatusEvent = 'wingmate:agent-status';
function subscribeToAgentError(listener: () => void) {
  window.addEventListener(agentStatusEvent, listener);
  return () => window.removeEventListener(agentStatusEvent, listener);
}

window.__ModuleLoader__.load({
  id: 'dsh-wingmate-backends',
  factory(require) {
    const { createElement: h, useEffect, useSyncExternalStore } = require('react') as typeof import('react');
    const { Button } = require('@deepseek-ai/dsh-client-ui-primitives') as { Button: Button };
    const agents = window.__WINGMATE_AGENTS__ ?? [];
    const unavailable = { getSnapshot: () => false, subscribe: () => () => {} };

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
        const diagnostics = createDiagnosticsSource();
        function useDiagnostics() {
          useEffect(() => { void diagnostics.request(); }, []);
          return useSyncExternalStore(diagnostics.subscribe, diagnostics.getSnapshot);
        }

        function openDiagnostics() {
          ctx.get('pluginNavigation')?.openList();
          if (ctx.slots.entries('plugins.tab').some(entry => entry.options.id === 'wingmate-preinstalled')) {
            ctx.slots.entries('main').find(entry => entry.options.key === 'plugins')?.store?.create().actions.setTab('wingmate-preinstalled');
          }
        }

        function AgentPicker() {
          const { rows } = useDiagnostics();
          const error = useSyncExternalStore(subscribeToAgentError, () => window.__WINGMATE_AGENT_ERROR__ ?? '');
          const missing = rows?.find(row => !row.adapterAvailable);
          useEffect(() => subscribeToAgentError(() => { void diagnostics.request(); }), []);
          return <div style={{ maxWidth: 220 }}>
            <select aria-label="执行后端" style={{ font: 'inherit', color: 'inherit', background: 'transparent', borderRadius: 6, padding: '4px 8px' }}
              value={agents.some(agent => agent.id === backend) ? backend ?? '' : ''}
              onChange={event => {
                const url = new URL(location.href);
                const target = event.currentTarget.value;
                if (target) url.searchParams.set('wingmate-agent', target);
                else url.searchParams.delete('wingmate-agent');
                url.hash = '';
                location.assign(url);
              }}>
              <option value="">DSH</option>
              {agents.map(agent => <option key={agent.id} value={agent.id}
                disabled={rows?.some(row => row.id === agent.id && !row.adapterAvailable)}>{agent.label}</option>)}
            </select>
            {error && <div role="alert" style={{ whiteSpace: 'normal', overflowWrap: 'anywhere', fontSize: 12 }}>{error}</div>}
            {(missing || error) && <div style={{ fontSize: 12 }}>
              {missing && <span>{missing.label} 适配器不可用</span>}
              <Button size="sm" variant="ghost" onClick={openDiagnostics}>查看连接设置</Button>
            </div>}
          </div>;
        }

        function DiagnosticsPanel() {
          const { rows, checking, readFailed } = useDiagnostics();
          return <div style={{ display: 'grid', gap: 8 }}>
            {!rows && !readFailed && <div role="status">正在读取连接状态…</div>}
            {readFailed && <div role="alert">无法读取连接状态，请重试。</div>}
            {agents.map(agent => {
              const row = rows?.find(item => item.id === agent.id);
              return <div key={agent.id} style={{ display: 'grid', gap: 6 }}>
                <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
                  <strong>{agent.label}</strong>
                  {row && <span role="status">{row.adapterAvailable
                    ? `已找到适配器 · ${connectionLabels[row.connection]}` : '适配器不可用'}</span>}
                  <Button size="sm" variant="outline" disabled={checking !== undefined}
                    onClick={() => { void diagnostics.request(agent.id); }}>{checking === agent.id ? '正在检查…' : '检查连接'}</Button>
                </div>
                {row?.detail && <div style={{ color: 'var(--dsw-alias-label-secondary)', overflowWrap: 'anywhere' }}>{row.detail}</div>}
                {row && !row.adapterAvailable && agent.id === 'codex' && <div>
                  在 Wingmate 主机上安装并登录 Codex，再运行 <code style={{ overflowWrap: 'anywhere' }}>npm install -g @agentclientprotocol/codex-acp@2.6.0</code>。
                </div>}
              </div>;
            })}
          </div>;
        }

        ctx.slots.inject('sidebar.footer.action', () => ctx.slots.register({
          name: 'sidebar.footer.action', id: 'wingmate-agent-picker',
        }, AgentPicker));
        ctx.slots.inject('wingmate.preinstalled.backends', () => ctx.slots.register({
          name: 'wingmate.preinstalled.backends',
        }, DiagnosticsPanel));
      },
    };
  },
});
