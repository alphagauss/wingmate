// The extensionless Unix launcher also runs in directories using ES modules.
import('node:child_process').then(({ spawnSync }) => {
  const result = spawnSync(executable, process.argv.slice(2), {
    stdio: 'inherit',
    env: { ...process.env, WINGMATE_HOME: home },
  });
  if (result.error?.code === 'ENOENT') {
    console.error('Wingmate moved. Register the terminal commands again in Wingmate settings.');
  } else if (result.error) {
    console.error(result.error.message);
  }
  process.exitCode = result.status ?? 1;
});
