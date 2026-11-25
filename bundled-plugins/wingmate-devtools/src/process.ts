import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, delimiter } from 'node:path';

export type Invocation = {
  command: string;
  args: readonly string[];
  env?: Readonly<Record<string, string>>;
};
export type Runner = (
  invocation: Invocation,
  args: string[],
  directory: string,
  signal: AbortSignal,
  log: (chunk: string, stream?: 'stdout' | 'stderr') => void,
) => Promise<string>;

export function packageCommand(name: 'npm' | 'pnpm'): Invocation {
  const entry = name === 'npm' ? 'npm/bin/npm-cli.js' : 'pnpm/bin/pnpm.mjs';
  for (const bin of [
    dirname(process.execPath),
    ...(process.env.PATH || '').split(delimiter),
  ]) {
    for (const root of [
      join(bin, 'node_modules'),
      join(bin, '../lib/node_modules'),
    ]) {
      const path = join(root, entry);
      if (existsSync(path)) return { command: process.execPath, args: [path] };
    }
  }
  throw new Error('未找到 ' + name + '，请检查 Wingmate 随包工具或开发机安装');
}

export const runCommand: Runner = async (
  invocation,
  args,
  directory,
  signal,
  log,
) => {
  signal.throwIfAborted();
  return new Promise<string>((resolve, reject) => {
    const child = spawn(invocation.command, [...invocation.args, ...args], {
      cwd: directory,
      env: {
        ...process.env,
        ...invocation.env,
        pnpm_config_verify_deps_before_run: 'false',
      },
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
      detached: process.platform !== 'win32',
    });
    let output = '';
    let termination: Promise<void> | undefined;
    let forceTimer: ReturnType<typeof setTimeout> | undefined;
    const abort = () => {
      if (!child.pid || termination) return;
      if (process.platform === 'win32') {
        termination = new Promise<void>((done) => {
          const killer = spawn(
            'taskkill.exe',
            ['/pid', String(child.pid), '/t', '/f'],
            { windowsHide: true, stdio: 'ignore' },
          );
          killer.once('error', () => {
            child.kill();
            done();
          });
          killer.once('close', () => done());
        });
      } else {
        const kill = (mode: NodeJS.Signals) => {
          try {
            process.kill(-child.pid!, mode);
          } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== 'ESRCH')
              child.kill(mode);
          }
        };
        kill('SIGTERM');
        forceTimer = setTimeout(() => kill('SIGKILL'), 1500);
      }
    };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => {
      output = (output + chunk).slice(-131_072);
      log(chunk, 'stdout');
    });
    child.stderr.on('data', (chunk: string) => log(chunk, 'stderr'));
    child.once('error', (error) => {
      signal.removeEventListener('abort', abort);
      reject(error);
    });
    child.once('close', async (code) => {
      await termination;
      if (forceTimer) clearTimeout(forceTimer);
      signal.removeEventListener('abort', abort);
      if (signal.aborted) reject(signal.reason);
      else if (code !== 0) reject(new Error('命令失败，退出状态 ' + code));
      else resolve(output);
    });
  });
};
