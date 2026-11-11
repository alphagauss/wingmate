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
const share = document.getElementById('share');
const sharing = document.getElementById('sharing');
const shareStatus = document.getElementById('share-status');
const shareEnable = document.getElementById('share-enable');
const sharePair = document.getElementById('share-pair');
const shareResult = document.getElementById('share-result');
const shareLink = document.getElementById('share-link');
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

async function updateShare(action) {
  const status = await invoke('local_share', { command: { action } });
  shareStatus.textContent = status.address ? 'http://' + status.address : '分享已关闭';
  shareEnable.hidden = Boolean(status.address);
  sharePair.hidden = !status.address;
  shareResult.hidden = !status.pairing_url;
  shareLink.value = status.pairing_url ?? '';
  const code = document.getElementById('share-code');
  if (status.qr) code.src = 'data:image/svg+xml,' + encodeURIComponent(status.qr);
  else code.removeAttribute('src');
}

share.addEventListener('click', () => act(share, async () => {
  if (panel.hidden) {
    await invoke('show_remote', { visible: false });
    panel.hidden = false;
    toggle.textContent = '返回工作台';
  }
  sharing.hidden = !sharing.hidden;
  if (!sharing.hidden) await updateShare('status');
}));
shareEnable.addEventListener('click', () => act(shareEnable, () => updateShare('enable')));
sharePair.addEventListener('click', () => act(sharePair, () => updateShare('pair')));
document.getElementById('share-copy').addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(shareLink.value);
    shareStatus.textContent = '配对链接已复制';
  } catch {
    shareLink.select();
    shareStatus.textContent = '请复制选中的配对链接';
  }
});
