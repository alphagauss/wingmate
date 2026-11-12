import type { ComponentType, ReactNode } from 'react';

type InventoryEntry = {
  moduleName: string;
  enabled: boolean;
  fiberPhase: 'pending' | 'loading' | 'active' | 'failed' | 'unloading' | null;
};
type InventoryResult = { ok: true; value: { entries: readonly InventoryEntry[] } } | { ok: false };
type Preview = { id: string; extensions: readonly string[] };
type PreviewRegistry = {
  getSnapshot(): readonly Preview[];
  subscribe(listener: () => void): () => void;
};
type ItemProps = {
  view: 'summary' | 'page';
  renderSlot(name: 'wingmate.preinstalled.backends', props: object): ReactNode;
};
type ClientContext = {
  get(name: 'documentPreviews'): PreviewRegistry | undefined;
  remote: { pluginInventory: { list(): Promise<InventoryResult> } };
  slots: {
    inject(name: 'plugins.item', register: () => Generator<() => void>): void;
    register(options: {
      name: 'plugins.item'; id: string; label: string; order: number;
      children?: { 'wingmate.preinstalled.backends': { kind: 'single'; scope: 'root' } };
    }, component: ComponentType<ItemProps>): () => void;
    entries(name: 'sidebar.settings'): readonly { store?: { create(): { actions: { openSection(id: string): void } } } }[];
  };
};
type Primitives = {
  Button: ComponentType<{ size: 'sm'; variant: 'outline'; onClick(): void; children: ReactNode }>;
  Tag: ComponentType<{ tone?: 'success' | 'warning'; children: ReactNode }>;
};

const plugins = [
  { id: 'dsh-wingmate-backends', entry: 'wingmate-backends/dist/host.mjs', title: 'Wingmate 执行后端', summary: '在侧栏底部切换 DSH 和 Codex。' },
  { id: 'dsh-wingmate-file-viewer', entry: 'wingmate-file-viewer/host.mjs', title: 'Wingmate 文件预览', summary: '在工作台的文件侧栏预览文档和媒体。' },
  { id: 'dsh-wingmate-web', entry: 'wingmate-web/host.mjs', title: 'Wingmate 后台服务', summary: '管理当前主机的后台服务与分享。' },
];
type Plugin = typeof plugins[number];
const noPreviews: readonly Preview[] = [];
const emptyRegistry: PreviewRegistry = { getSnapshot: () => noPreviews, subscribe: () => () => {} };
export const detailSection = { display: 'grid', gap: 12 };
export const detailHeading = { margin: 0, fontSize: 14, lineHeight: '20px', fontWeight: 600 };
const detailText = { color: 'var(--dsw-alias-label-secondary)', fontSize: 12, lineHeight: '18px' };

function statusOf(entries: readonly InventoryEntry[] | undefined, plugin: Plugin) {
  if (!entries) return { label: '状态未知' };
  const entry = entries.find(({ moduleName }) => moduleName.replaceAll('\\', '/').endsWith(`/${plugin.entry}`));
  if (!entry?.enabled) return { label: '未加载' };
  if (entry.fiberPhase === 'active') return { label: '已加载', tone: 'success' } as const;
  if (entry.fiberPhase === 'failed') return { label: '加载失败', tone: 'warning' } as const;
  return { label: '未就绪' };
}

export function registerPreinstalledItems(ctx: ClientContext, react: typeof import('react'),
  { Button, Tag }: Primitives, desktop: boolean, ShareSettings: ComponentType<{ openSettings?: () => void }>) {
  const { createElement: h, useEffect, useState, useSyncExternalStore } = react;

  function Details({ plugin, renderSlot }: { plugin: Plugin; renderSlot: ItemProps['renderSlot'] }) {
    const [entries, setEntries] = useState<readonly InventoryEntry[]>();
    const [loading, setLoading] = useState(true);
    const [refresh, setRefresh] = useState(0);
    const previewRegistry = (plugin.id === 'dsh-wingmate-file-viewer' ? ctx.get('documentPreviews') : undefined) ?? emptyRegistry;
    const previews = useSyncExternalStore(previewRegistry.subscribe, previewRegistry.getSnapshot);
    const extensions = previews.find(preview => preview.id === 'dsh-wingmate-file-viewer')?.extensions;

    useEffect(() => {
      let active = true;
      ctx.remote.pluginInventory.list()
        .then(result => { if (active && result.ok) setEntries(result.value.entries); })
        .catch(() => { /* Keep the status unknown on transport failure. */ })
        .finally(() => { if (active) setLoading(false); });
      return () => { active = false; };
    }, [refresh]);

    const settings = desktop && plugin.id === 'dsh-wingmate-web' ? ctx.slots.entries('sidebar.settings')[0]?.store : undefined;
    const status = statusOf(entries, plugin);
    return <div style={{ display: 'grid', gap: 26 }}>
      {plugin.id === 'dsh-wingmate-backends' && <section aria-label="连接检查" style={detailSection}>
        <h2 style={detailHeading}>连接检查</h2>
        {renderSlot('wingmate.preinstalled.backends', {})}
      </section>}
      {plugin.id === 'dsh-wingmate-file-viewer' && <section aria-label="支持格式" style={detailSection}>
        <h2 style={detailHeading}>支持格式</h2>
        {extensions?.length ? <ul style={{ display: 'flex', flexWrap: 'wrap', gap: 8, maxWidth: 520, listStyle: 'none', padding: 0, margin: 0 }}>
          {extensions.map(extension => <li key={extension} style={{
            ...detailText, padding: '3px 8px', borderRadius: 'var(--dsw-radius-sm, 8px)', background: 'var(--dsw-alias-interactive-bg-hover)',
          }}><code>.{extension}</code></li>)}
        </ul> : <span style={detailText}>尚未注册预览格式。</span>}
      </section>}
      {plugin.id === 'dsh-wingmate-web' && <ShareSettings openSettings={settings && (() => settings.create().actions.openSection('wingmate-backend'))} />}
      <div style={{ ...detailText, display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 8 }}>
        <span>插件状态</span>
        {loading ? <span role="status">正在读取…</span>
          : entries ? <Tag tone={status.tone}>{status.label}</Tag>
            : <>
              <span role="alert">无法读取</span>
              <Button size="sm" variant="outline" onClick={() => { setLoading(true); setRefresh(refresh + 1); }}>重试</Button>
            </>}
      </div>
    </div>;
  }

  ctx.slots.inject('plugins.item', function* () {
    for (const [index, plugin] of plugins.entries()) {
      yield ctx.slots.register({
        name: 'plugins.item', id: plugin.id, label: plugin.title, order: 100 + index,
        ...(plugin.id === 'dsh-wingmate-backends' ? {
          children: { 'wingmate.preinstalled.backends': { kind: 'single', scope: 'root' } },
        } : {}),
      }, ({ view, renderSlot }) => view === 'summary' ? plugin.summary : <Details plugin={plugin} renderSlot={renderSlot} />);
    }
  });
}
