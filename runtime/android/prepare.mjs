import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const build = ['bash', 'runtime/android/build-runtime.sh'];
const [command, ...args] = process.platform === 'win32' ? ['wsl.exe', '--exec', ...build] : build;
const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
if (result.error) console.error(`无法启动 Android 载荷准备（需要 Linux 或 WSL）：${result.error.message}`);
process.exitCode = result.status ?? 1;
