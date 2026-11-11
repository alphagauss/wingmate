import { statSync } from 'node:fs';
import { delimiter, isAbsolute, join } from 'node:path';

export function codexCommand() {
  const [command, ...args] = executable();
  const env = { ...process.env };
  // A Wingmate launched from Codex must open an independent Agent process.
  for (const key of ['CODEX_THREAD_ID', 'CODEX_SESSION_ID', 'CODEX_TOOL_CALL_ID',
    'CODEX_APP_TOOLS_PIPE_PATH', 'CODEX_APP_RUNTIME_CLI_PATH', 'CODEX_INTERNAL_ORIGINATOR_OVERRIDE',
    'CODEX_CI', 'CODEX_PERMISSION_PROFILE', 'CODEX_SANDBOX', 'CODEX_SANDBOX_NETWORK_DISABLED']) delete env[key];
  return { command, args, env };
}

function executable() {
  const configured = process.env.WINGMATE_CODEX_ACP;
  if (configured) {
    if (!isAbsolute(configured) || !isFile(configured)) {
      throw new Error('WINGMATE_CODEX_ACP must point to an existing executable or JavaScript entry');
    }
    return /\.[cm]?js$/i.test(configured) ? [process.execPath, configured] : [configured];
  }
  for (const path of (process.env.PATH ?? '').split(delimiter).filter(Boolean)) {
    if (process.platform === 'win32') {
      const native = join(path, 'codex-acp.exe');
      if (isFile(native)) return [native];
      // npm's Windows shim points to the package next to the global bin directory.
      const npm = join(path, 'node_modules', '@agentclientprotocol', 'codex-acp', 'dist', 'index.js');
      if (isFile(npm)) return [process.execPath, npm];
    } else {
      const native = join(path, 'codex-acp');
      if (isFile(native)) return [native];
    }
  }
  throw new Error('Codex ACP adapter not found. Install @agentclientprotocol/codex-acp on the host.');
}

function isFile(path) {
  return statSync(path, { throwIfNoEntry: false })?.isFile() === true;
}
