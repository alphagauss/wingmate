const { invoke } = window.__TAURI_INTERNALS__;
const header = document.querySelector('header');
const current = document.getElementById('current');
const toggle = document.getElementById('toggle');
const local = document.getElementById('local');
const panel = document.querySelector('main');
const form = document.querySelector('form');
const link = document.getElementById('link');
const hosts = document.getElementById('hosts');
const message = document.getElementById('message');
const key = 'wingmate-remote-hosts';

function savedHosts() {
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? '[]');
    return Array.isArray(saved) ? saved.filter(origin => typeof origin === 'string') : [];
  } catch {
    return [];
  }
}

function showSaved() {
  const saved = savedHosts();
  hosts.replaceChildren(new Option('选择主机', ''));
  for (const origin of saved) hosts.add(new Option(origin, origin));
  hosts.hidden = document.getElementById('saved-label').hidden = saved.length === 0;
}

function placeHeader() {
  header.toggleAttribute('data-windows', Boolean(window.__WINGMATE_TITLEBAR__));
}

placeHeader();
window.addEventListener('wingmate:titlebar', placeHeader);
showSaved();
hosts.addEventListener('change', () => {
  if (hosts.value) link.value = hosts.value;
});

async function act(button, operation) {
  button.disabled = true;
  message.textContent = '';
  try {
    await operation();
  } catch (error) {
    message.textContent = String(error);
  } finally {
    button.disabled = false;
  }
}

form.addEventListener('submit', event => {
  event.preventDefault();
  act(form.querySelector('button'), async () => {
    const { origin, transport } = await invoke('connect_remote', { link: link.value });
    current.textContent = (transport === 'lan' ? '局域网 · ' : '公网 · ') + new URL(origin).host;
    document.title = 'Wingmate · ' + current.textContent;
    link.value = origin;
    toggle.hidden = false;
    toggle.textContent = '连接';
    panel.hidden = true;
    localStorage.setItem(key, JSON.stringify([origin, ...savedHosts().filter(value => value !== origin)]));
    showSaved();
  });
});

toggle.addEventListener('click', () => {
  act(toggle, async () => {
    await invoke('show_remote', { visible: !panel.hidden });
    panel.hidden = !panel.hidden;
    toggle.textContent = panel.hidden ? '连接' : '返回工作台';
  });
});

local.addEventListener('click', () => act(local, () => invoke('return_local')));
