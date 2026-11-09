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
  update.hidden = actions !== 'update';
  actionError.hidden = true;
  for (const button of controls) button.disabled = waiting;
};

document.getElementById('show-restart').addEventListener('click', () => confirmation.showModal());
// The modal makes elements outside it inert, including the custom window controls.
confirmation.addEventListener('toggle', () => {
  const titlebar = document.querySelector('[data-wingmate-titlebar]');
  if (titlebar) (confirmation.open ? confirmation : document.body).append(titlebar);
});

for (const control of document.querySelectorAll('[data-backend-action]')) {
  control.addEventListener('click', async () => {
    if (confirmation.open) confirmation.close();
    actionError.hidden = true;
    for (const button of controls) button.disabled = true;
    try {
      await window.__TAURI_INTERNALS__.invoke('home_backend', { action: control.dataset.backendAction });
    } catch (error) {
      actionError.textContent = String(error);
      actionError.hidden = false;
      for (const button of controls) button.disabled = false;
    }
  });
}
