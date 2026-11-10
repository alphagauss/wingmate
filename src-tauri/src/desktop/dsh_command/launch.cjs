// The Unix launcher has no extension, so it must run under either module type.
async function main() {
  const { spawnSync } = await import('node:child_process');
  const { existsSync } = await import('node:fs');
  if (!existsSync(entry)) {
    console.error('DSH runtime moved. Register dsh again in Wingmate settings.');
    return 1;
  }
  const result = spawnSync(process.execPath, [entry, ...process.argv.slice(2)], {
    stdio: 'inherit',
    env: { ...process.env, DSH_HOME: dshHome, DSH_AGENTS_HOME: agentsHome },
  });
  if (result.error) console.error(result.error.message);
  return result.status ?? 1;
}

main().then(code => { process.exitCode = code; }, error => {
  console.error(error.message);
  process.exitCode = 1;
});
