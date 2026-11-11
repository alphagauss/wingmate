const { invoke } = window.__TAURI_INTERNALS__;
const { theme, origin, maxHeight } = window.wingmateConnections;
const panel = document.querySelector('main');
const tabs = [...document.querySelectorAll('[role="tab"]')];
const form = document.querySelector('form');
const link = document.getElementById('link');
const message = document.getElementById('message');
const shareStatus = document.getElementById('share-status');
const shareEnable = document.getElementById('share-enable');
const sharePair = document.getElementById('share-pair');
const shareResult = document.getElementById('share-result');
const shareLink = document.getElementById('share-link');
const shareCode = document.getElementById('share-code');
const shareCopy = document.getElementById('share-copy');
const key = 'wingmate-remote-hosts';

document.documentElement.dataset.theme = theme;
panel.style.maxHeight = `${maxHeight}px`;
new ResizeObserver(() => {
  invoke('resize_connections', { height: Math.ceil(panel.getBoundingClientRect().height) }).catch(report);
}).observe(panel);

function report(error) { message.textContent = String(error); }

function close() { invoke('close_connections').catch(report); }
document.getElementById('close').addEventListener('click', close);
document.addEventListener('keydown', event => {
  if (event.key === 'Escape' && !event.defaultPrevented) close();
});

function savedHosts() {
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? '[]');
    return Array.isArray(saved) ? saved.filter(address => typeof address === 'string') : [];
  } catch {
    return [];
  }
}

const saved = savedHosts();
if (saved.length) {
  const hosts = document.getElementById('saved');
  const trigger = document.getElementById('host-picker');
  const value = document.getElementById('host-value');
  const menu = document.getElementById('host-options');
  const clear = menu.firstElementChild;
  let selected = '';

  for (const address of saved) {
    const option = document.createElement('button');
    option.type = 'button';
    option.setAttribute('role', 'option');
    option.setAttribute('aria-selected', 'false');
    option.tabIndex = -1;
    option.dataset.address = address;
    option.textContent = address;
    menu.append(option);
  }
  hosts.hidden = false;

  function selectHost(address) {
    selected = address;
    value.textContent = address || '选择主机';
    clear.hidden = !address;
    for (const option of menu.children) {
      option.setAttribute('aria-selected', String(Boolean(address) && option.dataset.address === address));
    }
  }

  function dismissHosts() {
    if (menu.hidden) return;
    menu.hidden = true;
    trigger.setAttribute('aria-expanded', 'false');
    if (hosts.contains(document.activeElement)) document.activeElement.blur();
  }

  function openHosts() {
    menu.hidden = false;
    trigger.setAttribute('aria-expanded', 'true');
    return [...menu.children].filter(option => !option.hidden);
  }

  trigger.addEventListener('click', event => {
    if (!menu.hidden) dismissHosts();
    else {
      const options = openHosts();
      if (event.detail === 0) (options.find(option => option.dataset.address === selected) ?? options[0]).focus();
    }
  });
  menu.addEventListener('click', event => {
    const option = event.target.closest('[role="option"]');
    if (!option) return;
    const address = option.dataset.address;
    if (address || link.value === selected) link.value = address;
    dismissHosts();
    selectHost(address);
  });
  hosts.addEventListener('keydown', event => {
    const key = event.key;
    if (key === 'Escape' && !menu.hidden) {
      event.preventDefault();
      dismissHosts();
    } else if ((key === 'Enter' || key === ' ') && menu.contains(document.activeElement)) {
      event.preventDefault();
      document.activeElement.click();
    } else if (key === 'ArrowDown' || key === 'ArrowUp' || key === 'Home' || key === 'End') {
      event.preventDefault();
      const options = openHosts();
      const index = options.indexOf(document.activeElement);
      const last = options.length - 1;
      const next = {
        Home: 0, End: last,
        ArrowDown: index === last ? 0 : index + 1,
        ArrowUp: index <= 0 ? last : index - 1,
      }[key];
      options[next].focus();
    }
  });
  hosts.addEventListener('focusout', event => {
    if (!hosts.contains(event.relatedTarget)) dismissHosts();
  });
  document.addEventListener('pointerdown', event => {
    if (!hosts.contains(event.target)) dismissHosts();
  });
  link.addEventListener('input', () => {
    if (selected && selected !== link.value) selectHost('');
  });
}
if (origin) {
  const current = document.getElementById('current');
  current.textContent = '当前主机 · ' + new URL(origin).host;
  current.hidden = false;
}

async function act(button, operation) {
  if (button.disabled) return;
  button.disabled = true;
  message.textContent = '';
  try {
    await operation();
  } catch (error) {
    report(error);
  } finally {
    button.disabled = false;
  }
}

form.addEventListener('submit', event => {
  event.preventDefault();
  act(form.querySelector('button'), async () => {
    const address = await invoke('connect_remote', { link: link.value });
    localStorage.setItem(key, JSON.stringify([address, ...saved.filter(item => item !== address)]));
    close();
  });
});

async function updateShare(action) {
  const status = await invoke('local_share', { command: { action } });
  shareStatus.textContent = status.address ? 'http://' + status.address : '局域网分享已关闭';
  shareEnable.hidden = Boolean(status.address);
  sharePair.hidden = !status.address;
  shareResult.hidden = !status.pairing_url;
  shareLink.value = status.pairing_url ?? '';
  if (status.qr) shareCode.src = 'data:image/svg+xml,' + encodeURIComponent(status.qr);
  else shareCode.removeAttribute('src');
  shareCopy.textContent = '复制';
}

let shareStatusRequest;
function selectTab(tab) {
  for (const item of tabs) {
    const selected = item === tab;
    item.setAttribute('aria-selected', String(selected));
    item.tabIndex = selected ? 0 : -1;
    document.getElementById(item.getAttribute('aria-controls')).hidden = !selected;
  }
  message.textContent = '';
  if (tab.id === 'share-tab' && !shareStatusRequest) {
    shareStatus.textContent = '正在读取分享状态…';
    shareStatusRequest = updateShare('status').catch(error => {
      shareStatusRequest = undefined;
      shareStatus.textContent = '无法读取分享状态';
      report(error);
    });
  }
}

for (const tab of tabs) {
  tab.addEventListener('click', () => selectTab(tab));
  tab.addEventListener('keydown', event => {
    const target = event.key === 'Home' ? tabs[0]
      : event.key === 'End' ? tabs.at(-1)
      : ['ArrowLeft', 'ArrowRight'].includes(event.key) ? tabs.find(item => item !== tab) : null;
    if (!target) return;
    event.preventDefault();
    selectTab(target);
    target.focus();
  });
}

shareEnable.addEventListener('click', () => act(shareEnable, () => updateShare('enable')));
sharePair.addEventListener('click', () => act(sharePair, () => updateShare('pair')));
shareCopy.addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(shareLink.value);
    shareCopy.textContent = '已复制';
  } catch {
    shareLink.select();
    report('请复制选中的配对链接');
  }
});
