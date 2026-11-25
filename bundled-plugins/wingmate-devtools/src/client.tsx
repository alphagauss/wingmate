import React, { useEffect, useRef, useState } from 'react';
import type { BuildResult, Project } from './projects.ts';
import { apiPath } from './operations.ts';
import styles from './style.css';

export const name = 'dsh-wingmate-devtools';
export const inject = ['slots', 'remoteAccess'];
type Context = {
  remoteAccess: {
    whenAccess(action: string, install: () => Generator<() => void>): void;
  };
  slots: {
    inject(name: string, install: () => () => void): () => void;
    register(
      options: { name: string; id: string; label: string; order: number },
      component: React.ComponentType<{ view: string }>,
    ): () => void;
  };
};
type Row = Project & { building: boolean };
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
  const current = useRef<AbortController | null>(null);
  async function load(signal?: AbortSignal) {
    const response = await fetch(apiPath, { signal });
    const value = await response.json();
    if (!response.ok) throw new Error(value.error || '无法读取项目');
    setRows(value);
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
  async function run(action: string, target?: string) {
    if (current.current) return;
    const controller = new AbortController();
    current.current = controller;
    setBusy(target || action);
    setError('');
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
      <h2>独立插件开发</h2>
      <p>
        关联源码目录，开发完成后编译并更新。正在使用的插件保持为上次安装的成品。
      </p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run('register');
        }}
      >
        <label htmlFor="wingmate-plugin-source">源码目录</label>
        <div className="wingmate-devtools-actions">
          <input
            id="wingmate-plugin-source"
            value={directory}
            onChange={(event) => setDirectory(event.target.value)}
            placeholder="输入插件项目的绝对路径"
            disabled={!!busy}
            required
          />
          <button disabled={!!busy}>关联项目</button>
        </div>
      </form>
      <div className="wingmate-devtools-actions">
        <button
          disabled={!!busy}
          onClick={() => void load().catch((cause) => setError(cause.message))}
        >
          刷新项目
        </button>
        {busy && (
          <>
            <span role="status">正在处理 {busy}…</span>
            <button onClick={() => current.current?.abort()}>取消</button>
          </>
        )}
      </div>
      {!rows.length && (
        <p>尚未关联项目。可以在创造模式中创建插件，再把源码目录添加到这里。</p>
      )}
      {rows.map((row) => (
        <article key={row.name}>
          <h3>{row.name}</h3>
          <p className="wingmate-devtools-path">{row.directory}</p>
          <div className="wingmate-devtools-actions">
            <button
              disabled={!!busy || row.building}
              onClick={() => void run('build', row.name)}
            >
              编译并更新
            </button>
            <button
              disabled={!!busy || row.building}
              onClick={() => void run('forget', row.name)}
            >
              移除关联
            </button>
          </div>
        </article>
      ))}
      {error && <p role="alert">{error}</p>}
      {report && (
        <section aria-live="polite">
          <h3>
            {report.error
              ? (stages[report.stage] || report.stage) + '失败'
              : outcomes[report.result?.application || ''] || '请检查安装结果'}
          </h3>
          {report.error && <p role="alert">{report.error}</p>}
          {report.result && report.result.application !== 'applied' && (
            <pre>{JSON.stringify(report.result, null, 2)}</pre>
          )}
          <details>
            <summary>构建日志</summary>
            <pre>{report.log || '没有构建输出'}</pre>
          </details>
        </section>
      )}
    </section>
  );
}
export function apply(ctx: Context) {
  function Card({ view }: { view: string }) {
    return view === 'summary' ? (
      <p>关联独立插件源码，编译并更新当前环境中的插件。</p>
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
          order: 40,
        },
        Card,
      ),
    );
  });
}
