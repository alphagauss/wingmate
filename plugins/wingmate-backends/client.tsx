import type { ButtonHTMLAttributes, ComponentType, ReactNode, SVGProps } from 'react';
import { diagnosticsPath } from './diagnostics.ts';
import type { BackendStatus } from './diagnostics.ts';

type Agent = { id: string; label: string };
type Button = ComponentType<ButtonHTMLAttributes<HTMLButtonElement> & {
  size: 'sm'; variant: 'outline' | 'ghost';
}>;
type Icon = ComponentType<SVGProps<SVGSVGElement> & { size: number }>;
type MenuEntry = { id: string; label: string; disabled?: boolean } | { id: string; type: 'separator' };
type Menu = ComponentType<{
  open: boolean; anchor: ReactNode; items: readonly MenuEntry[]; selectedId: string;
  onSelect(id: string): void; onClose(): void; side: 'top' | 'right'; portal: boolean; autoFocus: boolean;
}>;
type Observable = { getSnapshot(): boolean; subscribe(listener: () => void): () => void };
type ClientModule = { inject: string[]; apply(ctx: ClientContext): void };
type ClientContext = {
  remoteAccess: { source(kind: string, id: string, action: string): Observable };
  effect(install: () => () => void, label: string): void;
  get(name: 'pluginNavigation'): { openList(): void } | undefined;
  slots: {
    inject(name: string, register: () => () => void): void;
    register<Props>(options: { name: string; id?: string }, component: ComponentType<Props>): () => void;
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
    const { createElement: h, useEffect, useState, useSyncExternalStore } = require('react') as typeof import('react');
    const { Button, Menu, IconChevronDownOutlineRegular, IconCodeOutlineRegular } = require('@deepseek-ai/dsh-client-ui-primitives') as {
      Button: Button; Menu: Menu; IconChevronDownOutlineRegular: Icon; IconCodeOutlineRegular: Icon;
    };
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

        function AgentPicker({ wide }: { wide: boolean }) {
          const [open, setOpen] = useState(false);
          const { rows } = useDiagnostics();
          const error = useSyncExternalStore(subscribeToAgentError, () => window.__WINGMATE_AGENT_ERROR__ ?? '');
          const missing = rows?.find(row => !row.adapterAvailable);
          const current = agents.find(agent => agent.id === backend) ?? { id: '', label: 'DSH' };
          useEffect(() => subscribeToAgentError(() => { void diagnostics.request(); }), []);
          const items: MenuEntry[] = [{ id: '', label: 'DSH' }];
          for (const agent of agents) {
            const disabled = rows?.some(row => row.id === agent.id && !row.adapterAvailable);
            items.push({ id: agent.id, label: disabled ? `${agent.label}（适配器不可用）` : agent.label, disabled });
          }
          if (missing || error) items.push({ type: 'separator', id: '$separator' }, { id: '$diagnostics', label: '打开 Wingmate 预装' });

          return <div style={{ maxWidth: 220 }}>
            <Menu open={open} autoFocus portal side={wide ? 'top' : 'right'} selectedId={current.id} items={items}
              onClose={() => setOpen(false)}
              onSelect={target => {
                setOpen(false);
                if (target === '$diagnostics') { openDiagnostics(); return; }
                if (target === current.id) return;
                const url = new URL(location.href);
                if (target) url.searchParams.set('wingmate-agent', target);
                else url.searchParams.delete('wingmate-agent');
                url.hash = '';
                location.assign(url);
              }}
              anchor={<Button size="sm" variant="ghost" aria-label="执行后端" aria-description={`当前：${current.label}`}
                aria-haspopup="menu" aria-expanded={open} title={`执行后端 · ${current.label}`}
                onClick={() => setOpen(!open)} onKeyDown={event => {
                  if (!open && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
                    event.preventDefault();
                    setOpen(true);
                  }
                }}>
                {wide ? <span style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
                  {current.label}<IconChevronDownOutlineRegular size={12} aria-hidden="true" />
                </span>
                  : <IconCodeOutlineRegular size={16} aria-hidden="true" />}
              </Button>} />
            {wide && (error || missing) && <div role={error ? 'alert' : 'status'}
              style={{ overflowWrap: 'anywhere', fontSize: 12, color: 'var(--dsw-alias-label-secondary)' }}>
              {error || `${missing?.label} 适配器不可用`}
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
