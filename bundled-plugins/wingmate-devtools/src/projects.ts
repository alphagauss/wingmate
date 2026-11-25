import { createHash, randomUUID } from 'node:crypto';
import {
  mkdir,
  readFile,
  realpath,
  rename,
  stat,
  writeFile,
  copyFile,
  mkdtemp,
  rm,
} from 'node:fs/promises';
import { basename, isAbsolute, join } from 'node:path';
import { packageCommand, runCommand } from './process.ts';
import type { Invocation, Runner } from './process.ts';

export type Project = { name: string; directory: string };
export type Bundle = {
  name: string;
  enabled: boolean;
  removable: boolean;
  source?: string;
};
export type Application = { application: string; [key: string]: unknown };
export type Manager = {
  listBundles(): Promise<Bundle[]>;
  installBundle(
    spec: string,
    options: { enabled: boolean; requestId: string },
  ): Promise<Application>;
  cancelInstall(requestId: string): Promise<unknown>;
  removeBundle(name: string): Promise<Application>;
  setBundleEnabled(name: string, enabled: boolean): Promise<Application>;
};
type Manifest = {
  name: string;
  version: string;
  scripts?: { build?: string };
  dsh?: { bundle?: { patch?: unknown } };
};
export type BuildResult = {
  name: string;
  stage: string;
  log: string;
  result?: Application;
  error?: string;
  archive?: string;
};
type Options = {
  storage: string;
  manager: Manager;
  packageManager?: Invocation;
  runner?: Runner;
};

export function createProjects({
  storage,
  manager,
  packageManager,
  runner = runCommand,
}: Options) {
  const file = join(storage, 'sources.json');
  let loaded: Promise<Project[]> | undefined;
  let writing = Promise.resolve();
  const running = new Set<string>();
  const pending = new Set<Promise<unknown>>();
  const lifetime = new AbortController();

  function read(): Promise<Project[]> {
    return (loaded ??= readFile(file, 'utf8')
      .then((text) => JSON.parse(text) as Project[])
      .catch((error) => {
        if (error.code === 'ENOENT') return [];
        throw error;
      }));
  }
  async function save(projects: Project[]) {
    await mkdir(storage, { recursive: true });
    const temporary = file + '.' + randomUUID() + '.tmp';
    await writeFile(temporary, JSON.stringify(projects, null, 2));
    try {
      await rename(temporary, file);
    } finally {
      await rm(temporary, { force: true });
    }
    loaded = Promise.resolve(projects);
  }
  function edit<T>(operation: () => Promise<T>): Promise<T> {
    const next = writing.then(operation);
    writing = next.then(
      () => {},
      () => {},
    );
    return next;
  }
  async function manifest(
    directory: string,
    buildRequired = true,
  ): Promise<Manifest> {
    const value = JSON.parse(
      await readFile(join(directory, 'package.json'), 'utf8'),
    ) as Manifest;
    if (
      typeof value.name !== 'string' ||
      !/^(?:@[a-z0-9._-]+\/)?[a-z0-9][a-z0-9._-]*$/.test(value.name) ||
      typeof value.version !== 'string' ||
      !value.dsh?.bundle?.patch ||
      (buildRequired && !value.scripts?.build)
    ) {
      throw new Error(
        '目录必须是声明名称、版本、dsh.bundle 和 build 命令的插件项目',
      );
    }
    const installed = (await manager.listBundles()).find(
      (bundle) => bundle.name === value.name,
    );
    if (
      value.name === 'dsh-wingmate-devtools' ||
      installed?.removable === false
    )
      throw new Error('随包插件通过 Wingmate 更新，请选择独立插件源码');
    return value;
  }

  async function install(spec: string, enabled: boolean, signal: AbortSignal) {
    signal.throwIfAborted();
    const requestId = randomUUID();
    let cancellation: Promise<unknown> | undefined;
    const cancel = () => {
      cancellation = manager.cancelInstall(requestId).catch(() => {});
    };
    signal.addEventListener('abort', cancel, { once: true });
    try {
      const operation = manager.installBundle(spec, { enabled, requestId });
      if (signal.aborted) cancel();
      return await operation;
    } finally {
      signal.removeEventListener('abort', cancel);
      await cancellation;
    }
  }

  async function build(
    name: string,
    caller: AbortSignal,
  ): Promise<BuildResult> {
    lifetime.signal.throwIfAborted();
    if (running.has(name)) throw new Error('这个插件正在编译更新');
    running.add(name);
    const signal = AbortSignal.any([
      caller,
      lifetime.signal,
      AbortSignal.timeout(10 * 60_000),
    ]);
    const report: BuildResult = { name, stage: 'prepare', log: '' };
    const log = (text: string) => {
      report.log = (report.log + text).slice(-32_768);
    };
    let temporary: string | undefined;
    try {
      const project = (await read()).find((item) => item.name === name);
      if (!project) throw new Error('请先关联插件源码目录');
      const before = await manifest(project.directory);
      if (before.name !== name)
        throw new Error('源码包名已变化，请重新关联目录');
      const pnpm = packageManager ?? packageCommand('pnpm');
      report.stage = 'dependencies';
      log('准备项目依赖\n');
      const locked = await stat(join(project.directory, 'pnpm-lock.yaml')).then(
        () => true,
        (error) => {
          if (error.code === 'ENOENT') return false;
          throw error;
        },
      );
      await runner(
        pnpm,
        ['install', ...(locked ? ['--frozen-lockfile'] : [])],
        project.directory,
        signal,
        log,
      );
      report.stage = 'build';
      log('\n构建正式产物\n');
      await runner(pnpm, ['run', 'build'], project.directory, signal, log);
      signal.throwIfAborted();
      const after = await manifest(project.directory);
      if (after.name !== name || after.version !== before.version)
        throw new Error('构建期间包名或版本发生变化，请重新执行');
      report.stage = 'pack';
      await mkdir(join(storage, 'packages'), { recursive: true });
      temporary = await mkdtemp(join(storage, 'packages', '.pack-'));
      const output = await runner(
        packageCommand('npm'),
        ['pack', '--ignore-scripts', '--json', '--pack-destination', temporary],
        project.directory,
        signal,
        log,
      );
      const packed = JSON.parse(output) as {
        name: string;
        version: string;
        filename: string;
      }[];
      if (
        packed.length !== 1 ||
        packed[0].name !== name ||
        packed[0].version !== before.version ||
        basename(packed[0].filename) !== packed[0].filename
      )
        throw new Error('打包结果与项目声明不一致');
      const source = join(temporary, packed[0].filename);
      const hash = createHash('sha256')
        .update(await readFile(source))
        .digest('hex');
      const archive = join(storage, 'packages', hash + '.tgz');
      await copyFile(source, archive);
      signal.throwIfAborted();
      report.stage = 'install';
      report.archive = archive;
      const installed = (await manager.listBundles()).find(
        (bundle) => bundle.name === name,
      );
      report.result = await install(
        archive,
        installed?.enabled ?? true,
        signal,
      );
      return report;
    } catch (error) {
      report.error = error instanceof Error ? error.message : String(error);
      return report;
    } finally {
      running.delete(name);
      if (temporary)
        await rm(temporary, { recursive: true, force: true }).catch((error) =>
          log('\n临时打包目录清理失败：' + error.message),
        );
    }
  }

  return {
    async preview(directory: string, caller: AbortSignal) {
      const signal = AbortSignal.any([caller, lifetime.signal]);
      signal.throwIfAborted();
      if (!isAbsolute(directory))
        throw new Error('请输入开发产物目录的绝对路径');
      const resolved = await realpath(directory);
      const value = await manifest(resolved, false);
      const operation = (async () => {
        const current = (await manager.listBundles()).find(
          (bundle) => bundle.name === value.name,
        );
        if (
          current?.source?.startsWith('link:') &&
          (await realpath(current.source.slice(5)).catch(() => undefined)) ===
            resolved
        ) {
          return manager.setBundleEnabled(value.name, true);
        }
        if (current) {
          const removed = await manager.removeBundle(value.name);
          if (removed.application !== 'applied') return removed;
        }
        return install('link:' + resolved, true, signal);
      })();
      pending.add(operation);
      try {
        return await operation;
      } finally {
        pending.delete(operation);
      }
    },
    async list() {
      return (await read()).map((project) => ({
        ...project,
        building: running.has(project.name),
      }));
    },
    register(directory: string) {
      return edit(async () => {
        if (!isAbsolute(directory)) throw new Error('请输入源码目录的绝对路径');
        const resolved = await realpath(directory);
        const value = await manifest(resolved);
        if (running.has(value.name))
          throw new Error('编译期间不能修改源码关联');
        const project = { name: value.name, directory: resolved };
        await save([
          ...(await read()).filter((item) => item.name !== value.name),
          project,
        ]);
        return project;
      });
    },
    forget(name: string) {
      return edit(async () => {
        if (running.has(name)) throw new Error('编译期间不能移除源码关联');
        await save((await read()).filter((item) => item.name !== name));
      });
    },
    build(name: string, signal: AbortSignal) {
      const operation = build(name, signal);
      pending.add(operation);
      void operation.finally(() => pending.delete(operation)).catch(() => {});
      return operation;
    },
    async dispose() {
      lifetime.abort(new Error('开发辅助插件已停止'));
      await Promise.allSettled([...pending, writing]);
    },
  };
}
