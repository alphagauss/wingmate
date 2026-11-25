import React from 'react';
import { Button } from '@deepseek-ai/dsh-client-ui-primitives';
import { App } from '../ui/App.tsx';
import { StatisticsIcon } from '../ui/controls.tsx';
import styles from '../ui/styles.css';
import { pluginName } from '../types.ts';

export const name = pluginName;
export const inject = ['slots', 'layout'];
type Context = {
  layout: { selectPanel(id: string): void };
  get(name: 'pluginNavigation'): { openBundle(name: string): void } | undefined;
  slots: {
    inject(name: string, install: () => () => void): void;
    register(
      options: {
        name: string;
        id?: string;
        key?: string;
        order?: number;
        label?: string;
      },
      component: React.ComponentType,
    ): () => void;
  };
};

export function apply(ctx: Context) {
  function Page() {
    const navigation = ctx.get('pluginNavigation');
    return (
      <>
        <style>{styles}</style>
        <App
          Button={Button}
          openDetails={
            navigation ? () => navigation.openBundle(name) : undefined
          }
        />
      </>
    );
  }
  function Icon() {
    return (
      <svg
        viewBox="0 0 24 24"
        width="18"
        height="18"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        aria-hidden="true"
      >
        <path d="M4 5h16M4 10h12M4 15h7M16 14v6M21 10v10M11 18v2" />
      </svg>
    );
  }
  function Details() {
    return (
      <section className="plugin-demo-details">
        <style>{styles}</style>
        <div className="plugin-demo-detail-info">
          <span className="plugin-demo-app-icon">
            <StatisticsIcon />
          </span>
          <div>
            <h3>开始一次文本统计</h3>
            <p>输入或粘贴文本，即可查看四项统计结果。</p>
          </div>
        </div>
        <Button
          variant="primary"
          size="sm"
          onClick={() => ctx.layout.selectPanel(name)}
        >
          打开示例页面
        </Button>
      </section>
    );
  }
  ctx.slots.inject('main', () =>
    ctx.slots.register({ name: 'main', key: name }, Page),
  );
  ctx.slots.inject('sidebar.panellist', () =>
    ctx.slots.register(
      { name: 'sidebar.panellist', id: name, label: '插件示例', order: 60 },
      Icon,
    ),
  );
  ctx.slots.inject('plugins.bundle.config', () =>
    ctx.slots.register({ name: 'plugins.bundle.config', key: name }, Details),
  );
}
