import { join } from 'node:path';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createProjects } from './projects.ts';
import type { Manager } from './projects.ts';
import type { Invocation } from './process.ts';
import { apiPath, executeOperation } from './operations.ts';

export const name = 'wingmate-devtools';
export const inject = [
  'connection',
  'pluginManager',
  'profileContext',
  'tools',
  'sandboxPolicy',
  'skills',
];
type Execution = {
  signal: AbortSignal;
  agent?: { session: unknown };
  callId: string;
};
type Approval = {
  request(input: {
    agent: object;
    toolName: string;
    callId: string;
    reason: string;
    signal: AbortSignal;
  }): Promise<string>;
};
type Context = {
  skills: {
    register(skill: {
      name: string;
      description: string;
      content: string;
      path: string;
      source: string;
      resourceBase: { kind: 'directory'; path: string };
    }): () => void;
  };
  effect(install: () => () => void | Promise<void>): void;
  profileContext: { dir: string; packageManager?: Invocation };
  pluginManager: Manager;
  get(name: 'approval'): Approval | undefined;
  sandboxPolicy: { resolve(input: { session?: unknown }): { mode: string } };
  connection: {
    fetch: {
      register(route: {
        path: string;
        methods: readonly string[];
        requestBody: 'buffered';
        fetch(request: Request): Promise<Response>;
      }): () => Promise<void>;
    };
  };
  tools: {
    register(tool: {
      name: string;
      description: string;
      parameters: object;
      output: {
        schema: object;
        render(args: unknown, value: string): { type: 'text'; text: string }[];
      };
      execute(input: unknown, exec: Execution): Promise<string>;
    }): () => void;
  };
};

export async function apply(ctx: Context) {
  const skillPath = new URL(
    '../skills/wingmate-plugin-development/SKILL.md',
    import.meta.url,
  );
  const skill = await readFile(skillPath, 'utf8');
  ctx.skills.register({
    name: 'wingmate-plugin-development',
    description: /^description: (.+)$/m.exec(skill)![1],
    content: skill.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n/, ''),
    path: fileURLToPath(skillPath),
    source: 'bundled',
    resourceBase: {
      kind: 'directory',
      path: fileURLToPath(new URL('../', import.meta.url)),
    },
  });
  const projects = createProjects({
    storage: join(ctx.profileContext.dir, 'wingmate-devtools'),
    manager: ctx.pluginManager,
    packageManager: ctx.profileContext.packageManager,
  });
  ctx.effect(() => () => projects.dispose());
  // Connection exact /api routes authenticate the request and reject non-owner Peers.
  ctx.connection.fetch.register({
    path: apiPath,
    methods: ['GET', 'POST'],
    requestBody: 'buffered',
    async fetch(request) {
      try {
        if (
          request.method === 'POST' &&
          request.headers.get('content-type')?.split(';')[0] !==
            'application/json'
        )
          return new Response(null, { status: 415 });
        const input =
          request.method === 'GET' ? { action: 'list' } : await request.json();
        return Response.json(
          await executeOperation(projects, input, request.signal),
          { headers: { 'Cache-Control': 'no-store' } },
        );
      } catch (error) {
        return Response.json(
          { error: error instanceof Error ? error.message : String(error) },
          { status: 400 },
        );
      }
    },
  });
  ctx.tools.register({
    name: 'wingmate_plugin',
    description:
      'Manage independent plugin source projects in this DSH profile. Register an absolute source directory, build and install compiled output, list projects, or forget a source association without uninstalling. Build runs dependency installation and package scripts with host permissions. Requires full access or approval. Read result.application: restart-required is not applied. Use the official plugin_manager for enabling, disabling and uninstalling.',
    parameters: {
      type: 'object',
      properties: {
        action: {
          type: 'string',
          enum: ['list', 'register', 'build', 'forget', 'preview'],
        },
        directory: {
          type: 'string',
          description:
            'Absolute source directory for register; compiled development package directory for preview in a development environment only.',
        },
        name: {
          type: 'string',
          description: 'Registered package name for build or forget.',
        },
      },
      required: ['action'],
      additionalProperties: false,
    },
    output: {
      schema: { type: 'string' },
      render: (_args, value) => [{ type: 'text', text: value }],
    },
    async execute(input, exec) {
      exec.signal.throwIfAborted();
      const mode = ctx.sandboxPolicy.resolve(
        exec.agent ? { session: exec.agent.session } : {},
      ).mode;
      if (mode !== 'danger-full-access') {
        const approval = ctx.get('approval');
        if (
          !['read-only', 'workspace-write'].includes(mode) ||
          !approval ||
          !exec.agent
        )
          throw new Error('插件开发操作需要完整权限或本次操作批准');
        const outcome = await approval.request({
          agent: exec.agent,
          toolName: 'wingmate_plugin',
          callId: exec.callId,
          reason:
            'escalate sandbox to danger-full-access: 构建和安装独立插件，执行源码项目脚本并修改当前 Profile。操作：' +
            JSON.stringify(input),
          signal: exec.signal,
        });
        if (outcome !== 'allowed-once')
          throw new Error('插件开发操作未获批准：' + outcome);
      }
      exec.signal.throwIfAborted();
      return JSON.stringify(
        await executeOperation(projects, input, exec.signal),
      );
    },
  });
}
