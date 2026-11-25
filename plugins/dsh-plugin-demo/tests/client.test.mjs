import { test } from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import { readFile } from 'node:fs/promises';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { pluginName, statisticsPath } from '../src/types.ts';
import { buildPlugin } from '../scripts/build.mjs';

test('实际客户端产物使用宿主 React，注册侧栏与两个页面并清理', async () => {
  let registration;
  vm.runInNewContext(
    await readFile(new URL('../dist/client.js', import.meta.url), 'utf8'),
    {
      window: {
        __ModuleLoader__: {
          load(value) {
            registration = value;
          },
        },
      },
    },
  );
  assert.equal(registration.id, pluginName);
  const client = registration.factory((id) => {
    if (id === 'react') return React;
    assert.equal(id, '@deepseek-ai/dsh-client-ui-primitives');
    return {
      Button: ({ variant, size, icon, children, ...props }) =>
        React.createElement('button', props, icon, children),
    };
  });
  const entries = new Map(),
    disposers = [];
  client.apply({
    layout: { selectPanel() {} },
    get() {
      return { openBundle() {} };
    },
    slots: {
      inject(_name, install) {
        disposers.push(install());
      },
      register(options, Component) {
        entries.set(options.name, { options, Component });
        return () => entries.delete(options.name);
      },
    },
  });
  assert.equal(
    entries.get('sidebar.panellist').options.id,
    entries.get('main').options.key,
  );
  assert.match(
    renderToStaticMarkup(React.createElement(entries.get('main').Component)),
    /文本统计/,
  );
  assert.match(
    renderToStaticMarkup(
      React.createElement(entries.get('plugins.bundle.config').Component),
    ),
    /打开示例页面/,
  );
  for (const dispose of disposers.reverse()) dispose();
  assert.equal(entries.size, 0);
});

test('宿主卸载时注销实际 HTTP 路由', async () => {
  const { apply } = await import('../dist/host.js');
  let route,
    disposed = false,
    cleanup;
  apply({
    effect(fn) {
      cleanup = fn();
    },
    webServer: {
      register(value) {
        route = value;
        return () => {
          disposed = true;
        };
      },
    },
  });
  assert.equal(route.path, statisticsPath);
  assert.equal(typeof route.handler, 'function');
  cleanup();
  assert.equal(disposed, true);
});

test('开发构建使用独立目录，不覆盖正式产物', async () => {
  const production = await readFile(
    new URL('../dist/client.js', import.meta.url),
  );
  await buildPlugin(true);
  assert.deepEqual(
    await readFile(new URL('../dist/client.js', import.meta.url)),
    production,
  );
  const manifest = JSON.parse(
    await readFile(
      new URL('../.dev/dsh/package.json', import.meta.url),
      'utf8',
    ),
  );
  assert.equal(manifest.name, pluginName);
  assert.equal(manifest.scripts, undefined);
  assert.equal(manifest.devDependencies, undefined);
  assert.ok(
    (await readFile(new URL('../.dev/dsh/dist/client.js', import.meta.url)))
      .length,
  );
});
