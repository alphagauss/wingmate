const { invoke } = window.__TAURI_INTERNALS__;
const { theme, origin, maxHeight } = window.wingmateConnections;
const panel = document.querySelector('main');
const tabs = [...document.querySelectorAll('[role="tab"]')];
const form = document.querySelector('form');
const link = document.getElementById('link');
const message = document.getElementById('message');
const localWebStatus = document.getElementById('local-web-status');
const localPair = document.getElementById('local-pair');
const shareStatus = document.getElementById('share-status');
const shareEnable = document.getElementById('share-enable');
const sharePair = document.getElementById('share-pair');
const shareResult = document.getElementById('share-result');
const shareLink = document.getElementById('share-link');
const shareCode = document.getElementById('share-code');
const shareCopy = document.getElementById('share-copy');
const shareAddress = document.getElementById('share-address');
const shareAddressPicker = document.getElementById('share-address-picker');
const shareAddressTrigger = shareAddressPicker.querySelector('summary');
const shareAddressValue = document.getElementById('share-address-value');
const shareAddressOptions = document.getElementById('share-address-options');
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
  // A new pairing invalidates the previous code even if its reply fails to arrive.
  if (action === 'pair') shareResult.hidden = true;
  const status = await invoke('local_share', { command: { action } });
  localWebStatus.textContent = status.local_address ? '本机浏览器' : '本机 Web 入口不可用';
  localPair.hidden = !status.local_address;
  shareStatus.textContent = status.address ? '局域网分享已开启' : '局域网分享已关闭';
  shareEnable.hidden = Boolean(status.address);
  sharePair.hidden = !status.address;
  renderPairings(status.pairings ?? []);
}

let selectedPairing;
let shareCopyTimer;

function resetShareCopy() {
  clearTimeout(shareCopyTimer);
  shareCopy.textContent = '复制链接';
  shareCopy.classList.remove('copied');
}

function selectPairing(pairing) {
  selectedPairing = pairing;
  shareLink.hidden = true;
  shareLink.textContent = '';
  message.textContent = '';
  resetShareCopy();
  if (!pairing) {
    shareCode.removeAttribute('src');
    return;
  }
  shareAddress.textContent = pairing.address;
  shareAddressValue.textContent = pairing.address;
  shareAddressTrigger.setAttribute('aria-label', `切换配对地址，当前 ${pairing.address}`);
  shareCode.alt = `${pairing.address} 的配对二维码`;
  shareCode.src = 'data:image/svg+xml,' + encodeURIComponent(pairing.qr);
  for (const option of shareAddressOptions.children) {
    option.setAttribute('aria-pressed', String(option.textContent === pairing.address));
  }
}

function renderPairings(entries) {
  const previousAddress = selectedPairing?.address;
  const pairings = entries.map(entry => ({ ...entry, address: new URL(entry.url).host }));
  shareResult.hidden = !pairings.length;
  sharePair.textContent = pairings.length ? '重新生成' : '生成配对码';
  shareAddressPicker.open = false;
  shareAddressPicker.hidden = pairings.length < 2;
  shareAddress.hidden = !shareAddressPicker.hidden;
  shareAddressOptions.replaceChildren();
  if (!shareAddressPicker.hidden) {
    for (const pairing of pairings) {
      const option = document.createElement('button');
      option.type = 'button';
      option.className = 'pairing-address';
      option.textContent = pairing.address;
      option.addEventListener('click', () => {
        selectPairing(pairing);
        shareAddressPicker.open = false;
        shareAddressTrigger.focus();
      });
      shareAddressOptions.append(option);
    }
  }
  selectPairing(pairings.find(pairing => pairing.address === previousAddress) ?? pairings[0]);
}

shareAddressPicker.addEventListener('keydown', event => {
  if (event.key !== 'Escape' || !shareAddressPicker.open) return;
  event.preventDefault();
  shareAddressPicker.open = false;
  shareAddressTrigger.focus();
});
shareAddressPicker.addEventListener('focusout', event => {
  if (!shareAddressPicker.contains(event.relatedTarget)) shareAddressPicker.open = false;
});
document.addEventListener('pointerdown', event => {
  if (!shareAddressPicker.contains(event.target)) shareAddressPicker.open = false;
});

let shareStatusRequest;
function selectTab(tab) {
  for (const item of tabs) {
    const selected = item === tab;
    item.setAttribute('aria-selected', String(selected));
    item.tabIndex = selected ? 0 : -1;
    document.getElementById(item.getAttribute('aria-controls')).hidden = !selected;
  }
  message.textContent = '';
  clearTimeout(tunnelPoll);
  if (tab.id === 'tunnel-tab' && !tunnelBusy) updateTunnel();
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
      : event.key === 'ArrowLeft' ? tabs.at(tabs.indexOf(tab) - 1)
      : event.key === 'ArrowRight' ? tabs[(tabs.indexOf(tab) + 1) % tabs.length] : null;
    if (!target) return;
    event.preventDefault();
    selectTab(target);
    target.focus();
  });
}

shareEnable.addEventListener('click', () => act(shareEnable, () => updateShare('enable')));
sharePair.addEventListener('click', () => act(sharePair, () => updateShare('pair')));
localPair.addEventListener('click', () => {
  if (localPair.hasAttribute('aria-disabled')) return;
  act(localPair, async () => {
    const status = await invoke('local_share', { command: { action: 'pair_local' } });
    try {
      await navigator.clipboard.writeText(status.pairing_url);
    } catch {
      throw new Error('复制本机配对链接失败，请重试。');
    }
    const label = localPair.textContent;
    localPair.textContent = '已复制';
    localPair.setAttribute('aria-disabled', 'true');
    setTimeout(() => {
      localPair.textContent = '5 分钟有效';
      setTimeout(() => {
        localPair.textContent = label;
        localPair.removeAttribute('aria-disabled');
      }, 3000);
    }, 1000);
  });
});
shareCopy.addEventListener('click', async () => {
  const pairing = selectedPairing;
  if (!pairing) return;
  try {
    await navigator.clipboard.writeText(pairing.url);
  } catch {
    if (pairing !== selectedPairing) return;
    shareLink.textContent = pairing.url;
    shareLink.hidden = false;
    const range = document.createRange();
    range.selectNodeContents(shareLink);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
    report('请复制选中的配对链接');
    return;
  }
  if (pairing !== selectedPairing) return;
  resetShareCopy();
  message.textContent = '';
  shareLink.hidden = true;
  shareLink.textContent = '';
  shareCopy.textContent = '已复制';
  shareCopy.classList.add('copied');
  shareCopyTimer = setTimeout(resetShareCopy, 1000);
});

const tunnelPanel = document.getElementById('tunnel');
const tunnelModes = [...tunnelPanel.querySelectorAll('[data-mode]')];
const tunnelStatus = document.getElementById('tunnel-status');
const tunnelProgress = document.getElementById('tunnel-progress');
const tunnelAddress = document.getElementById('tunnel-address');
const tunnelHint = document.getElementById('tunnel-hint');
const tunnelStart = document.getElementById('tunnel-start');
const tunnelStop = document.getElementById('tunnel-stop');
const tunnelPair = document.getElementById('tunnel-pair');
const tunnelSettings = document.getElementById('tunnel-settings');
const tunnelResult = document.getElementById('tunnel-result');
const tunnelCode = document.getElementById('tunnel-code');
const tunnelCopy = document.getElementById('tunnel-copy');
const tunnelLink = document.getElementById('tunnel-link');

let tunnel;
let tunnelMode;
let tunnelPairing;
let tunnelPoll;
let tunnelRequest = 0;
let tunnelBusy = false;
let tunnelReadError;

function showTunnelPairing(pairing) {
  tunnelPairing = pairing;
  tunnelResult.hidden = !pairing;
  tunnelLink.hidden = true;
  tunnelLink.textContent = '';
  tunnelCopy.textContent = '复制链接';
  if (pairing) tunnelCode.src = 'data:image/svg+xml,' + encodeURIComponent(pairing.qr);
  else tunnelCode.removeAttribute('src');
}

function renderTunnel() {
  const mode = tunnelMode ?? tunnel?.mode ?? 'quick';
  const running = Boolean(tunnel?.url);
  const progress = tunnel?.progress;
  const switching = running && tunnel.mode !== mode;
  const needsSettings = mode === 'named' && !tunnel?.hostname;
  const downloading = progress?.stage === 'download';
  const percent = downloading && progress.total ? Math.min(100, Math.floor((progress.received ?? 0) * 100 / progress.total)) : null;
  const stages = {
    check: '正在检查 cloudflared…',
    download: percent === null ? '已下载 ' + ((progress?.received ?? 0) / 1048576).toFixed(1) + ' MiB' : '正在下载 cloudflared · ' + percent + '%',
    extract: '正在解压 cloudflared…',
    verify: '正在校验 cloudflared…',
    connect: '正在连接 Cloudflare…',
  };
  const text = tunnelReadError ?? tunnel?.error ?? (progress ? stages[progress.stage] : running ? '隧道已开启' : tunnel ? '隧道已关闭' : '请更新 Wingmate 管理服务');
  if (tunnelStatus.textContent !== text) tunnelStatus.textContent = text;
  tunnelProgress.hidden = !downloading;
  if (percent === null) tunnelProgress.removeAttribute('value');
  else { tunnelProgress.max = 100; tunnelProgress.value = percent; }
  tunnelAddress.textContent = tunnel?.url ?? '';
  tunnelAddress.title = tunnelAddress.textContent;
  tunnelAddress.hidden = !running;
  tunnelHint.textContent = needsSettings ? '先在 Wingmate 后台服务详情页填写域名和 Tunnel Token。'
    : switching ? '切换会断开现有公网连接。'
    : mode === 'quick' ? '连接器重启后会分配新地址，需要重新配对。'
    : running ? '重启隧道会短暂断开公网连接。' : '';
  tunnelHint.hidden = !tunnelHint.textContent;
  tunnelStart.hidden = !tunnel || Boolean(progress) || needsSettings || (running && !switching && mode === 'quick');
  tunnelStart.textContent = switching ? '切换并重启' : running ? '重启隧道' : '开启隧道';
  tunnelStop.hidden = !tunnel?.mode;
  tunnelPair.hidden = !running;
  tunnelPair.textContent = tunnelPairing ? '重新生成' : '生成配对码';
  tunnelSettings.hidden = !needsSettings || !tunnel;
  for (const button of tunnelModes) {
    button.setAttribute('aria-pressed', String(button.dataset.mode === mode));
    button.disabled = tunnelBusy || Boolean(progress);
  }
  for (const button of [tunnelStart, tunnelStop, tunnelPair, tunnelSettings]) button.disabled = tunnelBusy;
}

async function updateTunnel(command = { action: 'status' }) {
  clearTimeout(tunnelPoll);
  const id = ++tunnelRequest;
  if (command.action !== 'status') {
    tunnelBusy = true;
    showTunnelPairing(null);
    message.textContent = '';
    renderTunnel();
  }
  try {
    const status = await invoke('local_share', { command });
    if (id !== tunnelRequest) return;
    tunnelReadError = undefined;
    if (status.tunnel?.url !== tunnel?.url) showTunnelPairing(null);
    tunnel = status.tunnel;
    if (command.action === 'tunnel_pair') showTunnelPairing(status.pairings[0]);
  } catch (error) {
    if (id !== tunnelRequest) return;
    if (command.action === 'status') tunnelReadError = String(error);
    else if (!tunnelPanel.hidden) report(error);
  } finally {
    if (id === tunnelRequest) {
      tunnelBusy = false;
      renderTunnel();
      if (!tunnelPanel.hidden) tunnelPoll = setTimeout(updateTunnel, tunnel?.progress ? 350 : 1500);
    }
  }
}

for (const button of tunnelModes) button.addEventListener('click', () => {
  tunnelMode = button.dataset.mode;
  renderTunnel();
});
tunnelStart.addEventListener('click', () => updateTunnel({ action: 'tunnel_start', mode: tunnelMode ?? tunnel?.mode ?? 'quick' }));
tunnelStop.addEventListener('click', () => updateTunnel({ action: 'tunnel_stop' }));
tunnelPair.addEventListener('click', () => updateTunnel({ action: 'tunnel_pair' }));
tunnelSettings.addEventListener('click', () => act(tunnelSettings, () => invoke('open_tunnel_settings')));
tunnelCopy.addEventListener('click', () => act(tunnelCopy, async () => {
  const pairing = tunnelPairing;
  if (!pairing) return;
  try {
    await navigator.clipboard.writeText(pairing.url);
    if (pairing === tunnelPairing) tunnelCopy.textContent = '已复制';
  } catch {
    if (pairing !== tunnelPairing) return;
    tunnelLink.textContent = pairing.url;
    tunnelLink.hidden = false;
    report('请手动复制配对链接');
  }
}));
