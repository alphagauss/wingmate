import { createWorkbench } from './projection.mjs';
import { codexCommand } from './codex.mjs';
import { installWorkbench } from './workbench.mjs';
import { installDiagnostics } from './diagnostics.ts';
import { projectIndex } from './project-index.mjs';
import { join } from 'node:path';
import { homedir } from 'node:os';

export const inject = ['connection', 'webServer', 'typertGateway'];

export async function apply(ctx) {
  const directory = process.env.DSH_HOME ?? join(homedir(), '.dsh');
  const codex = createWorkbench('codex', 'Codex', codexCommand,
    projectIndex(join(directory, 'storages', 'wingmate-backends', 'codex.json')));
  ctx.effect(() => () => codex.dispose(), 'wingmate-backends: ACP lifetime');
  const backends = new Map([['codex', codex]]);
  await installWorkbench(ctx, backends);
  installDiagnostics(ctx, backends);
}
