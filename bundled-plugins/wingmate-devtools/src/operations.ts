import type { createProjects } from './projects.ts';
export const apiPath = '/api/wingmate-devtools';

export async function executeOperation(
  projects: ReturnType<typeof createProjects>,
  payload: unknown,
  signal: AbortSignal,
) {
  signal.throwIfAborted();
  if (!payload || typeof payload !== 'object' || !('action' in payload))
    throw new Error('缺少操作');
  const input = payload as {
    action?: unknown;
    directory?: unknown;
    name?: unknown;
  };
  switch (input.action) {
    case 'list':
      return projects.list();
    case 'register':
      if (typeof input.directory !== 'string' || !input.directory.trim())
        throw new Error('缺少源码目录');
      return projects.register(input.directory);
    case 'build':
      if (typeof input.name !== 'string' || !input.name)
        throw new Error('缺少插件名称');
      return projects.build(input.name, signal);
    case 'forget':
      if (typeof input.name !== 'string' || !input.name)
        throw new Error('缺少插件名称');
      await projects.forget(input.name);
      return { removed: input.name };
    default:
      throw new Error('不支持的操作');
  }
}
