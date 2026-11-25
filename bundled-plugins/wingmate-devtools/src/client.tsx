import React, { useEffect, useRef, useState } from 'react';
import {
  Button,
  Input,
  MenuItemButton,
  Modal,
  Tag,
} from '@deepseek-ai/dsh-client-ui-primitives';
import type { BuildResult, Project } from './projects.ts';
import { apiPath } from './operations.ts';
import styles from './style.css';

export const name = 'dsh-wingmate-devtools';
export const inject = ['slots', 'remoteAccess', 'pluginNavigation'];
type Context = {
  pluginNavigation: { openList(): void };
  remoteAccess: {
    whenAccess(action: string, install: () => Generator<() => void>): void;
  };
  slots: {
    inject(name: string, install: () => () => void): () => void;
    register<Props>(
      options: { name: string; id: string; label: string; order: number },
      component: React.ComponentType<Props>,
    ): () => void;
    entries(name: 'main'): readonly {
      options: { key?: string };
      store?: {
        create(): {
          actions: { setView(view: { kind: 'item'; id: string }): void };
        };
      };
    }[];
  };
};
type Row = Project & { building: boolean; latestLine?: string };
const outcomes: Record<string, string> = {
  applied: '已安装并应用',
  'restart-required': '已安装，重启 DSH 后生效',
  failed: '安装或加载失败',
  overridden: '已保存，但被其他配置覆盖',
  cancelled: '安装已取消',
};
const stages: Record<string, string> = {
  prepare: '检查项目',
  dependencies: '准备依赖',
  build: '编译',
  pack: '打包',
  install: '安装',
};

function Page() {
  const [rows, setRows] = useState<Row[]>([]);
  const [directory, setDirectory] = useState('');
  const [busy, setBusy] = useState('');
  const [error, setError] = useState('');
  const [report, setReport] = useState<BuildResult>();
  const [loading, setLoading] = useState(true);
  const [logOpen, setLogOpen] = useState(false);
  const current = useRef<AbortController | null>(null);
  const latestLine = rows.find((row) => row.name === busy)?.latestLine;
  const building = rows.some((row) => row.building);
  async function load(signal?: AbortSignal, quiet = false) {
    if (!quiet) setLoading(true);
    try {
      const response = await fetch(apiPath, { signal });
      const value = await response.json();
      if (!response.ok) throw new Error(value.error || '无法读取项目');
      if (!signal?.aborted) setRows(value);
    } finally {
      if (!quiet) setLoading(false);
    }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal).catch((cause) => {
      if (!controller.signal.aborted) setError(String(cause.message));
    });
    return () => {
      controller.abort();
      current.current?.abort();
    };
  }, []);
  useEffect(() => {
    if (!busy && !building) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        await load(controller.signal, true);
      } catch {
        // The build request reports failures; a missed progress refresh can retry.
      } finally {
        if (!controller.signal.aborted) timer = setTimeout(poll, 400);
      }
    };
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [busy, building]);
  async function run(action: string, target?: string) {
    if (current.current) return;
    const controller = new AbortController();
    current.current = controller;
    setBusy(target || action);
    setError('');
    setLogOpen(false);
    setReport(undefined);
    try {
      const response = await fetch(apiPath, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(
          action === 'register'
            ? { action, directory }
            : { action, name: target },
        ),
        signal: controller.signal,
      });
      const value = await response.json();
      if (!response.ok) throw new Error(value.error || '操作失败');
      if (action === 'build') setReport(value);
      if (action === 'register') setDirectory('');
      await load(controller.signal);
    } catch (cause) {
      setError(
        controller.signal.aborted
          ? '操作已取消，请刷新确认安装状态。'
          : cause instanceof Error
            ? cause.message
            : '操作失败',
      );
    } finally {
      current.current = null;
      setBusy('');
    }
  }
  return (
    <section className="wingmate-devtools">
      <style>{styles}</style>
      <form
        className="wingmate-devtools-connect"
        onSubmit={(event) => {
          event.preventDefault();
          void run('register');
        }}
      >
        <div>
          <label htmlFor="wingmate-plugin-source">关联源码项目</label>
          <p>选择已有插件的源码目录，开发完成后在这里编译并更新。</p>
        </div>
        <div className="wingmate-devtools-actions">
          <Input
            className="wingmate-devtools-input"
            id="wingmate-plugin-source"
            value={directory}
            onChange={(event) => setDirectory(event.target.value)}
            placeholder="输入插件项目的绝对路径"
            disabled={!!busy}
            required
            autoComplete="off"
            spellCheck={false}
          />
          <Button
            type="submit"
            variant="primary"
            disabled={!!busy || !directory.trim()}
          >
            关联项目
          </Button>
        </div>
      </form>
      <div className="wingmate-devtools-heading">
        <h2>
          源码项目 <span>{rows.length}</span>
        </h2>
        <Button
          size="sm"
          variant="ghost"
          disabled={!!busy || loading}
          onClick={() =>
            void load()
              .then(() => setError(''))
              .catch((cause) => setError(cause.message))
          }
        >
          {loading ? '刷新中…' : '刷新项目'}
        </Button>
      </div>
      {loading && !rows.length ? (
        <div className="wingmate-devtools-empty" role="status">
          <span className="wingmate-devtools-spinner" />
          正在读取项目…
        </div>
      ) : (
        !rows.length && (
          <div className="wingmate-devtools-empty">
            <ProjectIcon />
            <strong>从一个插件项目开始</strong>
            <p>在创造模式中创建插件，或关联已有的源码目录。</p>
          </div>
        )
      )}
      <div className="wingmate-devtools-projects">
        {rows.map((row) => (
          <article key={row.name} className="wingmate-devtools-project">
            <div className="wingmate-devtools-project-info">
              <span className="wingmate-devtools-project-icon">
                <ProjectIcon />
              </span>
              <div>
                <h3>{row.name}</h3>
                <p className="wingmate-devtools-path">{row.directory}</p>
              </div>
            </div>
            <div className="wingmate-devtools-actions">
              <Tag tone="neutral">
                {row.building || busy === row.name ? '处理中' : '已关联'}
              </Tag>
              <Button
                size="sm"
                variant="primary"
                disabled={!!busy || row.building}
                onClick={() => void run('build', row.name)}
              >
                编译并更新
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={!!busy || row.building}
                onClick={() => void run('forget', row.name)}
              >
                移除关联
              </Button>
            </div>
          </article>
        ))}
      </div>
      <p className="wingmate-devtools-hint">
        源码修改不会自动替换已安装版本。移除关联会保留源码和已安装插件。
      </p>
      {busy && (
        <div className="wingmate-devtools-progress" role="status">
          <span className="wingmate-devtools-spinner" />
          <span className="wingmate-devtools-progress-label">
            正在处理，请稍候…
          </span>
          {latestLine && (
            <span
              className="wingmate-devtools-progress-line"
              title={latestLine}
            >
              {latestLine}
            </span>
          )}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => current.current?.abort()}
          >
            取消
          </Button>
        </div>
      )}
      {error && (
        <p className="wingmate-devtools-error" role="alert">
          {error}
        </p>
      )}
      {report && (
        <section className="wingmate-devtools-report" aria-live="polite">
          <div className="wingmate-devtools-heading">
            <h3>更新结果</h3>
            <div className="wingmate-devtools-actions">
              <Tag
                tone={
                  report.unchanged
                    ? 'neutral'
                    : !report.error && report.result?.application === 'applied'
                      ? 'success'
                      : 'warning'
                }
              >
                {report.error
                  ? (stages[report.stage] || report.stage) + '失败'
                  : report.unchanged
                    ? '内容未变化，无需更新'
                    : outcomes[report.result?.application || ''] ||
                      '请检查安装结果'}
              </Tag>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setLogOpen(true)}
              >
                查看构建日志
              </Button>
            </div>
          </div>
          {report.error && <p role="alert">{report.error}</p>}
          {report.result && report.result.application !== 'applied' && (
            <pre>{JSON.stringify(report.result, null, 2)}</pre>
          )}
          <Modal
            open={logOpen}
            onClose={() => setLogOpen(false)}
            title="构建日志"
            closeLabel="关闭构建日志"
            description={report.name}
            className="wingmate-devtools-log-modal"
            contentClassName="wingmate-devtools-log-content"
          >
            <pre
              className="wingmate-devtools-log-output"
              tabIndex={0}
              data-modal-autofocus
              aria-label="构建输出"
            >
              {report.log || '没有构建输出'}
            </pre>
          </Modal>
        </section>
      )}
    </section>
  );
}
function ProjectIcon({ size = 20 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
      <path d="m9 11-3 3 3 3m6-6 3 3-3 3" />
    </svg>
  );
}
export function apply(ctx: Context) {
  function Card({ view }: { view: string }) {
    return view === 'summary' ? (
      <>关联独立插件源码，编译并更新当前环境中的插件。</>
    ) : (
      <Page />
    );
  }
  ctx.remoteAccess.whenAccess('pluginManager/installBundle', function* () {
    yield ctx.slots.inject('plugins.item', () =>
      ctx.slots.register(
        {
          name: 'plugins.item',
          id: name,
          label: 'Wingmate 插件开发',
          order: 41,
        },
        Card,
      ),
    );
    yield ctx.slots.inject('sidebar.panellist.actions', () =>
      ctx.slots.register(
        {
          name: 'sidebar.panellist.actions',
          id: name,
          label: 'Wingmate 插件开发',
          order: 100,
        },
        ({ onDismiss }: { onDismiss(): void }) => (
          <MenuItemButton
            icon={<ProjectIcon size={16} />}
            onSelect={() => {
              onDismiss();
              ctx.pluginNavigation.openList();
              ctx.slots
                .entries('main')
                .find((entry) => entry.options.key === 'plugins')
                ?.store?.create()
                .actions.setView({ kind: 'item', id: name });
            }}
          >
            Wingmate 插件开发
          </MenuItemButton>
        ),
      ),
    );
  });
}
