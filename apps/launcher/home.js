const confirmation = document.getElementById('restart-confirmation');
const actionError = document.getElementById('action-error');
const controls = document.querySelectorAll('[data-backend-action], #show-restart');
const messageText = document.getElementById('message');
const hintText = document.getElementById('hint');
const spinner = document.getElementById('spinner');
const details = document.getElementById('details');
const errorText = document.getElementById('error');
const start = document.getElementById('start');
const update = document.getElementById('update');
const showRestart = document.getElementById('show-restart');
const connectLocal = document.getElementById('connect-local');
const local = window.__WINGMATE_LOCAL_HOME__ === true;

function showActionError(error) {
  actionError.textContent = String(error);
  actionError.hidden = false;
}

window.renderWingmateHome = ({ message, hint, waiting, error, actions }) => {
  messageText.textContent = message;
  hintText.textContent = hint;
  hintText.hidden = !hint;
  spinner.hidden = !waiting;
  details.hidden = error === null;
  if (error === null) details.open = false;
  errorText.textContent = error ?? '';
  start.hidden = actions !== 'retry' && actions !== 'start';
  start.textContent = actions === 'retry' ? '重试' : '启动后台';
  const idleUpdate = actions === 'update-idle';
  update.hidden = actions !== 'update' && !idleUpdate;
  if (!update.hidden) {
    update.querySelector('[data-backend-action="continue"]').textContent = idleUpdate ? '继续并启动' : '继续使用';
    showRestart.textContent = idleUpdate ? '更新并启动' : '重启并更新';
    document.getElementById('restart-title').textContent = idleUpdate ? '更新管理服务并启动后台？' : '重启并更新后台？';
    document.getElementById('restart-hint').textContent = idleUpdate ? '已启用的远程连接会短暂中断。' : '这会中断正在运行的任务。';
    confirmation.querySelector('[data-backend-action="restart"]').textContent = idleUpdate ? '确认更新' : '确认重启';
  }
  actionError.hidden = true;
  for (const button of controls) button.disabled = waiting;
};

if (local) {
  connectLocal.hidden = false;
  connectLocal.addEventListener('click', () => {
    window.__TAURI_INTERNALS__.invoke('open_connections').catch(showActionError);
  });
} else {
  document.documentElement.dataset.remoteLauncher = '';
  let phase;
  let pending = false;
  const state = {
    stopped: ['后台已停止', '启动后将自动打开工作台。', false, 'start'],
    starting: ['正在启动后台…', '', true, ''],
    stopping: ['正在停止后台…', '', true, ''],
    unavailable: ['暂时无法读取主机状态', '正在重新连接。', true, ''],
  };
  async function refresh() {
    try {
      const response = await fetch('/_wingmate/status', { signal: AbortSignal.timeout(5000) });
      if (response.status === 401) {
        window.renderWingmateHome({
          message: '连接已失效', hint: '请从主机获取新的配对链接。', waiting: false, error: null, actions: '',
        });
        return;
      }
      if (!response.ok) throw new Error();
      const { phase: next } = await response.json();
      if (!pending || next === 'running') render(next);
    } catch {
      if (!pending) render('unavailable');
    }
    setTimeout(refresh, 1800);
  }
  function render(next) {
    if (next === 'running') {
      location.replace('/');
      return;
    }
    if (phase === next) return;
    phase = next;
    const [message, hint, waiting, actions] = state[next] ?? state.unavailable;
    window.renderWingmateHome({ message, hint, waiting: waiting || pending, error: null, actions });
  }
  start.addEventListener('click', async () => {
    if (pending) return;
    pending = true;
    render('starting');
    try {
      const response = await fetch('/_wingmate/manage', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action: 'start' }),
      });
      if (!response.ok) throw new Error(await response.text());
      render((await response.json()).phase);
    } catch (error) {
      pending = false;
      render('stopped');
      showActionError(error);
    } finally {
      pending = false;
      start.disabled = phase !== 'stopped';
    }
  });
  refresh();
}

showRestart.addEventListener('click', () => confirmation.showModal());
// The modal makes elements outside it inert, including the custom window controls.
confirmation.addEventListener('toggle', () => {
  const titlebar = document.querySelector('[data-wingmate-titlebar]');
  if (titlebar) (confirmation.open ? confirmation : document.body).append(titlebar);
});

for (const control of local ? document.querySelectorAll('[data-backend-action]') : []) {
  control.addEventListener('click', async () => {
    if (confirmation.open) confirmation.close();
    actionError.hidden = true;
    for (const button of controls) button.disabled = true;
    try {
      await window.__TAURI_INTERNALS__.invoke('home_backend', { action: control.dataset.backendAction });
    } catch (error) {
      showActionError(error);
      for (const button of controls) button.disabled = false;
    }
  });
}
