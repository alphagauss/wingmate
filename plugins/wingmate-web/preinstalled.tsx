import type { ButtonHTMLAttributes, ComponentType, ReactNode, SVGProps } from 'react';

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
type ClientContext = {
  get(name: 'documentPreviews'): PreviewRegistry | undefined;
  remote: { pluginInventory: { list(): Promise<InventoryResult> } };
  slots: { entries(name: 'sidebar.settings'): readonly {
    store?: { create(): { actions: { openSection(id: string): void } } };
  }[] };
};
type Icon = ComponentType<SVGProps<SVGSVGElement> & { size: number }>;
type Primitives = {
  Button: ComponentType<ButtonHTMLAttributes<HTMLButtonElement> & { size: 'sm'; variant: 'outline' | 'ghost' }>;
  Tag: ComponentType<{ tone?: 'success' | 'warning'; children: ReactNode }>;
  IconChevronLeftOutlineMedium: Icon;
  IconChevronRightOutlineRegular: Icon;
};

const plugins = [
  { id: 'dsh-wingmate-backends', entry: 'wingmate-backends/dist/host.mjs', title: '执行后端', detail: '在侧栏底部切换 DSH 和 Codex。' },
  { id: 'dsh-wingmate-file-viewer', entry: 'wingmate-file-viewer/host.mjs', title: '文件预览', detail: '在工作台的文件侧栏预览文档和媒体。' },
  { id: 'dsh-wingmate-web', entry: 'wingmate-web/host.mjs', title: '后台服务', detail: '在 Wingmate 桌面端查看后台状态、停止或重启后台。' },
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

export function createPreinstalledTab(ctx: ClientContext, react: typeof import('react'),
  { Button, Tag, IconChevronLeftOutlineMedium, IconChevronRightOutlineRegular }: Primitives, desktop: boolean) {
  const { createElement: h, useEffect, useState, useSyncExternalStore } = react;

  return function PreinstalledTab({ renderSlot }: { renderSlot(name: 'wingmate.preinstalled.backends', props: object): ReactNode }) {
    const [entries, setEntries] = useState<readonly InventoryEntry[]>();
    const [loading, setLoading] = useState(true);
    const [refresh, setRefresh] = useState(0);
    const [view, setView] = useState<{ detail?: Plugin; focus?: string }>({});
    const previewRegistry = ctx.get('documentPreviews') ?? emptyRegistry;
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

    const settings = desktop ? ctx.slots.entries('sidebar.settings')[0]?.store : undefined;
    function statusTag(plugin: Plugin) {
      const status = statusOf(entries, plugin);
      return loading ? null : <Tag tone={status.tone}>{status.label}</Tag>;
    }

    const detail = view.detail;
    return <div style={{ display: 'grid', gap: 12, paddingBlock: 12 }}>
      {loading && <div role="status">正在读取插件状态…</div>}
      {!loading && !entries && <div role="alert" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        无法读取插件状态。
        <Button size="sm" variant="outline" onClick={() => { setLoading(true); setRefresh(refresh + 1); }}>重试</Button>
      </div>}
      {detail ? <section aria-label={detail.title} style={{ display: 'grid', gap: 12 }}>
        <div>
          <Button size="sm" variant="ghost" autoFocus onClick={() => setView({ focus: detail.id })}>
            <IconChevronLeftOutlineMedium size={14} aria-hidden="true" />
            返回预装列表
          </Button>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <h2 style={{ margin: 0, fontSize: 18 }}>{detail.title}</h2>
          {statusTag(detail)}
        </div>
        <div style={{ color: 'var(--dsw-alias-label-secondary)' }}>{detail.detail}</div>
        {detail.id === 'dsh-wingmate-backends' && renderSlot('wingmate.preinstalled.backends', {})}
        {detail.id === 'dsh-wingmate-file-viewer' && extensions && <div>
          支持格式：{extensions.map(extension => `.${extension}`).join('、')}
        </div>}
        {detail.id === 'dsh-wingmate-web' && settings && <div>
          <Button size="sm" variant="outline" onClick={() => settings.create().actions.openSection('wingmate-backend')}>
            打开后台服务设置
          </Button>
        </div>}
      </section> : <ul aria-label="Wingmate 预装插件" style={{ display: 'grid', gap: 8, listStyle: 'none', margin: 0, padding: 0 }}>
        {plugins.map(plugin => <li key={plugin.id}>
          <Button size="sm" variant="outline" autoFocus={view.focus === plugin.id} onClick={() => setView({ detail: plugin })}
            style={{ width: '100%', height: 'auto', justifyContent: 'space-between', gap: 12, padding: '12px', textAlign: 'start' }}>
            <span style={{ display: 'grid', gap: 2 }}>
              <strong style={{ fontSize: 14 }}>{plugin.title}</strong>
              <span style={{ color: 'var(--dsw-alias-label-secondary)' }}>{plugin.detail}</span>
            </span>
            <span style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
              {statusTag(plugin)}
              <IconChevronRightOutlineRegular size={14} aria-hidden="true" />
            </span>
          </Button>
        </li>)}
      </ul>}
    </div>;
  };
}
