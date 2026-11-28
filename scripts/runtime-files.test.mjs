import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import * as tar from 'tar';
import { excludedRuntimeFile } from './runtime-files.mjs';

test('裁剪归档保留运行入口、类型、许可证和目标平台二进制', async () => {
  const root = await mkdtemp(join(tmpdir(), 'wingmate-archive-'));
  const kept = [
    'lib/index.js',
    'lib/index.d.ts',
    'lib/index.d.cts',
    'src/index.ts',
    'LICENSE',
    'data/model.onnx',
    'prebuilds/win32-x64/addon.node',
    'prebuilds/win32-arm64/LICENSE',
    'prebuilds/unknown/addon.node',
  ];
  const removed = [
    'lib/index.js.map',
    'lib/index.d.mts.map',
    'lib/index.d.cts.map',
    'lib/style.css.map',
    'prebuilds/win32-x64/addon.pdb',
    'prebuilds/win32-arm64/addon.node',
    'prebuilds/linux-x64/addon.node',
  ];
  try {
    for (const path of [...kept, ...removed]) {
      const full = join(root, 'source/node_modules/sample', path);
      await mkdir(join(full, '..'), { recursive: true });
      await writeFile(full, path);
    }
    const file = join(root, 'runtime.tar.gz');
    await tar.create(
      {
        cwd: join(root, 'source'),
        file,
        gzip: true,
        filter: (path, entry) =>
          !entry.isFile() || !excludedRuntimeFile(path, 'win32', 'x64'),
      },
      ['node_modules'],
    );
    const included = [];
    await tar.list({
      file,
      onReadEntry: (entry) => {
        if (entry.type === 'File')
          included.push(entry.path.replace('node_modules/sample/', ''));
      },
    });
    assert.deepEqual(included.sort(), kept.sort());
    assert.ok(
      (await readdir(join(root, 'source/node_modules/sample/lib'))).includes(
        'index.js.map',
      ),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('平台裁剪对 Windows 路径和其他目标架构一致', () => {
  assert.equal(
    excludedRuntimeFile(
      'node_modules/p/prebuilds/win32-x64/a.node',
      'linux',
      'x64',
    ),
    'foreign-prebuild',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules\\p\\prebuilds\\win32-arm64\\a.node',
      'win32',
      'arm64',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/p/prebuilds/linux-x64/node.napi.musl.node',
      'linux',
      'x64',
    ),
    undefined,
  );
});

test('所有目标排除 LibreOffice 包及嵌套引擎，保留许可证和无关 WASM', () => {
  for (const [platform, arch] of [
    ['win32', 'x64'],
    ['darwin', 'arm64'],
    ['linux', 'x64'],
    ['linux', 'arm64'],
  ]) {
    for (const pkg of [
      'libreoffice-kit',
      'libreoffice-kit-win32-x64',
      'libreoffice-kit-darwin-arm64',
      'libreoffice-kit-wasm',
    ]) {
      for (const prefix of [
        'node_modules',
        'node_modules/parent/node_modules',
      ]) {
        const root = `${prefix}/@deepseek-ai/${pkg}`;
        assert.equal(
          excludedRuntimeFile(`${root}/assets/soffice.wasm`, platform, arch),
          'libreoffice',
        );
        assert.equal(
          excludedRuntimeFile(
            `${root}/index.js`.replaceAll('/', '\\'),
            platform,
            arch,
          ),
          'libreoffice',
        );
        assert.equal(
          excludedRuntimeFile(`${root}/LICENSE`, platform, arch),
          undefined,
        );
      }
    }
    assert.equal(
      excludedRuntimeFile('node_modules/other/soffice.wasm', platform, arch),
      undefined,
    );
  }
});

test('排除重复的 ripgrep、内嵌 DevTools、冗余依赖文件和源码重复静态资源，保留 TS 源码与许可证', () => {
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-experimental-inspector/lib/devtools/entrypoints/lighthouse_worker/lighthouse_worker.js',
    ),
    'experimental-inspector',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-experimental-inspector/LICENSE',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-experimental-inspector-profile/cordis.patch.yml',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-experimental-client-ui-stats/lib/client.js',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile('node_modules/@vscode/ripgrep/lib/index.js'),
    'bundled-ripgrep',
  );
  assert.equal(
    excludedRuntimeFile('node_modules/@vscode/ripgrep-win32-x64/bin/rg.exe'),
    'bundled-ripgrep',
  );
  assert.equal(
    excludedRuntimeFile('node_modules/@vscode/ripgrep-win32-x64/LICENSE'),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/node-pty/third_party/conpty/1.25.260303002/win10-x64/OpenConsole.exe',
    ),
    'node-pty-third-party',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/node-pty/prebuilds/win32-x64/conpty/OpenConsole.exe',
      'win32',
      'x64',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@mixmark-io/domino/test/html5lib-tests.json',
    ),
    'package-dev-files',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@mixmark-io/domino/.yarn/plugins/@yarnpkg/plugin-version.cjs',
    ),
    'package-dev-files',
  );
  assert.equal(
    excludedRuntimeFile('node_modules/@mixmark-io/domino/lib/index.js'),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-client-ui-settings-account/src/client/assets/onboarding-welcome.webp',
    ),
    'duplicate-source-asset',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-client-ui-theme/src/styles/montserrat-regular.woff2',
    ),
    'duplicate-source-asset',
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-client-ui-settings-account/lib/assets/onboarding-welcome-8bee260fc9a4.webp',
    ),
    undefined,
  );
  assert.equal(
    excludedRuntimeFile(
      'node_modules/@deepseek-ai/dsh-client-ui-settings-account/src/client/index.tsx',
    ),
    undefined,
  );
});
