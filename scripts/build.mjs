// 先构建独立组件，再将其与运行归档一起内嵌到无 GUI 的发行入口。
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const headless = process.argv.includes('--headless');
if (process.argv.slice(2).some(arg => arg !== '--headless')) throw new Error('用法: build.mjs [--headless]');
function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
if (headless) {
  run('cargo', ['run', '--quiet', '-p', 'wingmate-launcher', '--no-default-features',
    '--bin', 'wingmate-runtime', '--', 'prepare-build']);
} else {
  run(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'build', '--no-bundle']);
  mkdirSync('work/build/bin', { recursive: true });
  const filename = `wingmate-desktop${process.platform === 'win32' ? '.exe' : ''}`;
  copyFileSync(join('target/release', filename), join('work/build/bin', filename));
}
run('cargo', ['build', '-p', 'wingmate-launcher', '--release', '--bin', 'wingmate',
  ...(headless ? ['--no-default-features', '--features', 'bundled-runtime'] : [])]);
