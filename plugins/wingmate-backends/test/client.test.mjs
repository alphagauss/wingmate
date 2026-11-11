import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';

test('external sessions withdraw DSH feedback and fork controls without changing native sessions', async () => {
  let module;
  const window = {
    __WINGMATE_AGENTS__: [{ id: 'codex' }],
    __ModuleLoader__: { load(entry) { module = entry.factory(() => ({})); } },
  };
  runInNewContext(await readFile(new URL('../dist/client.js', import.meta.url), 'utf8'),
    { window, URL, location: { href: 'http://localhost/?wingmate-agent=codex' } });
  const native = { getSnapshot: () => true };
  const source = () => native;
  let dispose;
  const ctx = {
    remoteAccess: { source },
    effect(install) { dispose = install(); },
    slots: { inject() {} },
  };
  module.apply(ctx);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'messageFeedback/put').getSnapshot(), false);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'session/fork').getSnapshot(), false);
  assert.equal(ctx.remoteAccess.source('session', 'dsh-session', 'messageFeedback/put'), native);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'session/prompt'), native);
  dispose();
  assert.equal(ctx.remoteAccess.source, source);
});

test('the sidebar renders with the React runtime and menu supplied by DSH', async () => {
  let picker;
  const react = {
    createElement: (type, props, ...children) => ({ type, props, children }),
    useEffect() {},
    useState: value => [value],
    useSyncExternalStore: (_subscribe, getSnapshot) => getSnapshot(),
  };
  const primitives = { Menu: 'Menu', Button: 'Button', IconChevronDownOutlineRegular: 'Chevron' };
  const window = {
    __WINGMATE_AGENTS__: [{ id: 'codex', label: 'Codex' }],
    __ModuleLoader__: { load(entry) {
      entry.factory(name => name === 'react' ? react : primitives).apply({ slots: {
        inject(_name, install) { install(); },
        register({ id }, component) { if (id === 'wingmate-agent-picker') picker = component; },
      } });
    } },
  };
  runInNewContext(await readFile(new URL('../dist/client.js', import.meta.url), 'utf8'),
    { window, URL, location: { href: 'http://localhost/' } });
  const menu = picker({ wide: true }).children[0];
  assert.equal(menu.type, 'Menu');
  assert.equal(menu.props.anchor.type, 'Button');
  assert.equal(menu.props.anchor.props['aria-description'], '当前：DSH');
});
