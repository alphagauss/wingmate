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
