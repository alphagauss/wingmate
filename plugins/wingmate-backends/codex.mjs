import { statSync } from 'node:fs';
import { isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';

export function codexCommand() {
  const [command, ...args] = executable();
  const env = { ...process.env };
  if (!env.WINGMATE_CODEX_ACP) env.CODEX_PATH ||= 'codex';
  // A Wingmate launched from Codex must open an independent Agent process.
  for (const key of ['CODEX_THREAD_ID', 'CODEX_SESSION_ID', 'CODEX_TOOL_CALL_ID',
    'CODEX_APP_TOOLS_PIPE_PATH', 'CODEX_APP_RUNTIME_CLI_PATH', 'CODEX_INTERNAL_ORIGINATOR_OVERRIDE',
    'CODEX_CI', 'CODEX_PERMISSION_PROFILE', 'CODEX_SANDBOX', 'CODEX_SANDBOX_NETWORK_DISABLED']) delete env[key];
  return { command, args, env };
}

function executable() {
  const configured = process.env.WINGMATE_CODEX_ACP;
  // The built host and the standalone adapter are shipped together in dist.
  const adapter = configured || fileURLToPath(new URL('./codex-acp.mjs', import.meta.url));
  if (!isAbsolute(adapter) || !isFile(adapter)) {
    throw new Error(configured
      ? 'WINGMATE_CODEX_ACP must point to an existing executable or JavaScript entry'
      : '内置 Codex ACP 适配器缺失，请重新构建或更新 Wingmate 运行包。');
  }
  return /\.[cm]?js$/i.test(adapter) ? [process.execPath, adapter] : [adapter];
}

function isFile(path) {
  return statSync(path, { throwIfNoEntry: false })?.isFile() === true;
}
