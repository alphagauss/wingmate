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
  { id: 'dsh-wingmate-web', entry: 'wingmate-web/host.mjs', title: 'Wingmate 后台服务', summary: '在 Wingmate 桌面端查看后台状态、停止或重启后台。' },
];
type Plugin = typeof plugins[number];
const noPreviews: readonly Preview[] = [];
const emptyRegistry: PreviewRegistry = { getSnapshot: () => noPreviews, subscribe: () => () => {} };

function statusOf(entries: readonly InventoryEntry[] | undefined, plugin: Plugin) {
  if (!entries) return { label: '状态未知' };
  const entry = entries.find(({ moduleName }) => moduleName.replaceAll('\\', '/').endsWith(`/${plugin.entry}`));
  if (!entry?.enabled) return { label: '未加载' };
  if (entry.fiberPhase === 'active') return { label: '已加载', tone: 'success' } as const;
  if (entry.fiberPhase === 'failed') return { label: '加载失败', tone: 'warning' } as const;
  return { label: '未就绪' };
}

export function registerPreinstalledItems(ctx: ClientContext, react: typeof import('react'),
  { Button, Tag }: Primitives, desktop: boolean) {
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
    return <div style={{ display: 'grid', gap: 12 }}>
      {loading && <div role="status">正在读取插件状态…</div>}
      {!loading && entries && <div><Tag tone={status.tone}>{status.label}</Tag></div>}
      {!loading && !entries && <div role="alert" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        无法读取插件状态。
        <Button size="sm" variant="outline" onClick={() => { setLoading(true); setRefresh(refresh + 1); }}>重试</Button>
      </div>}
      {plugin.id === 'dsh-wingmate-backends' && renderSlot('wingmate.preinstalled.backends', {})}
      {extensions && <div>支持格式：{extensions.map(extension => `.${extension}`).join('、')}</div>}
      {settings && <div>
        <Button size="sm" variant="outline" onClick={() => settings.create().actions.openSection('wingmate-backend')}>
          打开后台服务设置
        </Button>
      </div>}
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
