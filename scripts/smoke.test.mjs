import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempDisposable, writeFile, readFile, readdir, access, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';

test('冒烟拒绝非隔离 EXE，失败后保留日志，仅在确认停止后清理副本', async () => {
  await using fixture = await mkdtempDisposable(join(tmpdir(), 'wingmate-smoke-test-'));
  const source = join(fixture.path, 'fixture.rs');
  const exe = join(fixture.path, process.platform === 'win32' ? 'fixture.exe' : 'fixture');
  // A native CLI fixture exercises the real spawn/exit path without starting a background service.
  await writeFile(source, String.raw`
use std::{env,fs,path::PathBuf};
fn main() {
    let home = PathBuf::from(env::var("WINGMATE_HOME").unwrap());
    let mode = env::var("SMOKE_FIXTURE_MODE").unwrap();
    let action = env::args().nth(1).unwrap();
    let active = home.join("active");
    if mode != "foreign" { fs::create_dir_all(&home).unwrap(); }
    if action == "start" {
        fs::write(env::var("SMOKE_FIXTURE_MARKER").unwrap(), "started").unwrap();
        if mode != "foreign" {
            fs::write(&active, "active").unwrap();
            fs::write(home.join("service.log"), "fixture diagnostic\n").unwrap();
        }
    }
    if action == "stop" {
        if mode == "stop-failure" { eprintln!("fixture stop failed"); std::process::exit(1); }
        let _ = fs::remove_file(&active);
    }
    if active.exists() {
        println!("{}", r#"{"phase":"running","instance":"fixture","pid":1,"engine_pid":1}"#);
    } else { println!("{}", r#"{"phase":"stopped"}"#); }
}
`);
  const built = spawnSync('rustc', ['--crate-name', 'smoke_fixture', '-C', 'debuginfo=0', source, '-o', exe],
    { encoding: 'utf8', windowsHide: true, timeout: 30_000 });
  assert.equal(built.status, 0, built.error?.message || built.stderr);
  for (const mode of ['foreign', 'failure', 'stop-failure', 'keep']) {
    const work = join(fixture.path, mode), marker = join(fixture.path, `${mode}.started`);
    const run = spawnSync(process.execPath, ['scripts/smoke.mjs', exe, work, ...(mode === 'keep' ? ['--keep-work'] : [])],
      { env: { ...process.env, SMOKE_FIXTURE_MODE: mode, SMOKE_FIXTURE_MARKER: marker },
        encoding: 'utf8', windowsHide: true, timeout: 30_000 });
    assert.equal(run.status, 1, run.error?.message || run.stderr || run.stdout);
    const report = JSON.parse(await readFile(join(work, 'result.json'), 'utf8'));
    const files = (await readdir(work)).sort();
    assert.equal(report.status, 'failed');
    assert.equal(report.retained, mode === 'keep' || mode === 'stop-failure');
    if (mode === 'foreign') {
      assert.match(report.error, /未使用隔离管理目录/);
      await assert.rejects(access(marker), { code: 'ENOENT' });
      assert.deepEqual(files, ['result.json']);
    } else {
      assert.equal(await readFile(join(work, 'service.log'), 'utf8'), 'fixture diagnostic\n');
      if (mode === 'failure') assert.deepEqual(files, ['result.json', 'service.log']);
      else assert.ok(files.includes(process.platform === 'win32' ? 'wingmate.exe' : 'wingmate'));
      if (mode === 'stop-failure') assert.match(report.cleanupError, /fixture stop failed/);
    }
  }
  const existing = join(fixture.path, 'existing');
  await mkdir(existing);
  await writeFile(join(existing, 'keep.txt'), 'user data');
  const refused = spawnSync(process.execPath, ['scripts/smoke.mjs', exe, existing],
    { encoding: 'utf8', windowsHide: true, timeout: 10_000 });
  assert.equal(refused.status, 1);
  assert.deepEqual(await readdir(existing), ['keep.txt']);
  assert.equal(await readFile(join(existing, 'keep.txt'), 'utf8'), 'user data');
});
