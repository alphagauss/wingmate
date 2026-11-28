const { invoke } = window.__TAURI_INTERNALS__;
const { theme, address, anchor, passwordOnly } = window.wingmateConnections;
const panel = document.querySelector('dialog');
const tabs = [...document.querySelectorAll('[role="tab"]')];
const form = document.querySelector('form');
const link = document.getElementById('link');
const hostPicker = document.getElementById('host-picker');
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
if (passwordOnly) {
  document.documentElement.dataset.passwordOnly = '';
  document.getElementById('heading').textContent = '管理局域网密码';
}
document.documentElement.style.setProperty('--anchor-top', `${anchor.top}px`);
document.documentElement.style.setProperty(
  '--anchor-right',
  `${anchor.right}px`,
);
panel.showModal();

function report(error) {
  message.textContent = String(error);
}

let closing = false;
async function close(restoreFocus = true) {
  if (closing) return;
  closing = true;
  const animation =
    !restoreFocus || matchMedia('(prefers-reduced-motion: reduce)').matches
      ? null
      : panel.animate(
          [{ opacity: 1 }, { opacity: 0, transform: 'translateY(-4px)' }],
          { duration: 120, easing: 'ease-in', fill: 'forwards' },
        );
  try {
    await animation?.finished;
    await invoke('close_connections', { restoreFocus });
  } catch (error) {
    animation?.cancel();
    closing = false;
    report(error);
  }
}
document.getElementById('close').addEventListener('click', () => close());
window.addEventListener('blur', () => {
  if (!connectionBusy) close(false);
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && !event.defaultPrevented) {
    event.preventDefault();
    close();
  }
});
panel.addEventListener('cancel', (event) => {
  event.preventDefault();
  close();
});
document.addEventListener('pointerdown', (event) => {
  const { left, right, top, bottom } = panel.getBoundingClientRect();
  if (
    event.clientX < left ||
    event.clientX > right ||
    event.clientY < top ||
    event.clientY > bottom
  ) {
    close();
  }
});

function legacyHosts() {
  try {
    const saved = JSON.parse(localStorage.getItem(key) ?? '[]');
    return Array.isArray(saved)
      ? saved.filter((address) => typeof address === 'string')
      : [];
  } catch {
    return [];
  }
}

const recent = passwordOnly
  ? Promise.resolve([])
  : invoke('connection_history', { legacy: legacyHosts() })
      .then((hosts) => {
        try {
          localStorage.removeItem(key);
        } catch {
          // The shared history remains available without WebView storage.
        }
        return hosts;
      })
      .catch(legacyHosts);

recent.then((saved) => {
  if (!saved.length) return;
  const hosts = document.getElementById('saved');
  const trigger = hostPicker;
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
      option.setAttribute(
        'aria-selected',
        String(Boolean(address) && option.dataset.address === address),
      );
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
    return [...menu.children].filter((option) => !option.hidden);
  }

  trigger.addEventListener('click', (event) => {
    if (!menu.hidden) dismissHosts();
    else {
      const options = openHosts();
      if (event.detail === 0)
        (
          options.find((option) => option.dataset.address === selected) ??
          options[0]
        ).focus();
    }
  });
  menu.addEventListener('click', (event) => {
    const option = event.target.closest('[role="option"]');
    if (!option || connectionBusy) return;
    const address = option.dataset.address;
    if ((address || link.value === selected) && link.value !== address) {
      link.value = address;
      link.dispatchEvent(new Event('input'));
    }
    dismissHosts();
    selectHost(address);
  });
  hosts.addEventListener('keydown', (event) => {
    const key = event.key;
    if (key === 'Escape' && !menu.hidden) {
      event.preventDefault();
      dismissHosts();
    } else if (
      (key === 'Enter' || key === ' ') &&
      menu.contains(document.activeElement)
    ) {
      event.preventDefault();
      document.activeElement.click();
    } else if (
      key === 'ArrowDown' ||
      key === 'ArrowUp' ||
      key === 'Home' ||
      key === 'End'
    ) {
      event.preventDefault();
      const options = openHosts();
      const index = options.indexOf(document.activeElement);
      const last = options.length - 1;
      const next = {
        Home: 0,
        End: last,
        ArrowDown: index === last ? 0 : index + 1,
        ArrowUp: index <= 0 ? last : index - 1,
      }[key];
      options[next].focus();
    }
  });
  hosts.addEventListener('focusout', (event) => {
    if (!hosts.contains(event.relatedTarget)) dismissHosts();
  });
  document.addEventListener('pointerdown', (event) => {
    if (!hosts.contains(event.target)) dismissHosts();
  });
  link.addEventListener('input', () => {
    if (selected && selected !== link.value) selectHost('');
  });
});
if (address) {
  const current = document.getElementById('current');
  current.textContent =
    '当前主机 · ' + (address.includes('://') ? new URL(address).host : address);
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

function passwordEye(input, toggle) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'password-eye';
  button.setAttribute('aria-controls', input.id);
  button.innerHTML =
    '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 12s3.6-6 10-6 10 6 10 6-3.6 6-10 6S2 12 2 12Z"/><circle cx="12" cy="12" r="2.5"/><path class="eye-slash" d="m4 4 16 16"/></svg>';
  function setVisible(visible) {
    input.type = visible ? 'text' : 'password';
    button.setAttribute('aria-pressed', String(visible));
    button.setAttribute('aria-label', visible ? '隐藏密码' : '显示密码');
  }
  setVisible(false);
  button.addEventListener('click', () =>
    toggle ? toggle(setVisible, button) : setVisible(input.type === 'password'),
  );
  input.after(button);
  return setVisible;
}

const manualAuth = document.getElementById('manual-auth');
const lanSearch = document.getElementById('scan-lan');
const lanStatus = document.getElementById('lan-search-status');
const lanHosts = document.getElementById('lan-hosts');
const lanLoginTemplate = document.getElementById('lan-login-template');
let scanGeneration = 0;
let activeScan;
let connectionBusy = false;
let selectedHost;
let activeLogin;

function clearLogin() {
  selectedHost?.setAttribute('aria-expanded', 'false');
  selectedHost = undefined;
  activeLogin?.remove();
  activeLogin = undefined;
}

async function connectRemote(
  address,
  password,
  row,
  errorStatus = message,
  requestedAddress = address,
) {
  if (connectionBusy) return;
  connectionBusy = true;
  link.disabled = hostPicker.disabled = true;
  errorStatus.textContent = '';
  try {
    const result = await invoke('connect_remote', {
      link: address,
      password,
      requested: requestedAddress,
    });
    if (result.needs_password || result.needs_pairing) {
      clearLogin();
      const item = lanLoginTemplate.content.firstElementChild.cloneNode(true);
      const login = item.querySelector('form');
      const input = login.querySelector('input');
      const button = login.querySelector('[type="submit"]');
      const error = login.querySelector('.input-error');
      login.hidden = !result.needs_password;
      item.querySelector('.pair-required').hidden = !result.needs_pairing;
      selectedHost = row;
      activeLogin = item;
      if (row) {
        row.setAttribute('aria-expanded', 'true');
        row.after(item);
      } else manualAuth.append(item);
      if (result.needs_password) {
        passwordEye(input);
        login.addEventListener('submit', async (event) => {
          event.preventDefault();
          button.disabled = true;
          await connectRemote(
            result.origin,
            input.value,
            row,
            error,
            requestedAddress,
          );
          button.disabled = false;
        });
        input.focus();
      }
      return;
    }
    close();
  } catch (error) {
    errorStatus.textContent = String(error);
  } finally {
    connectionBusy = false;
    link.disabled = hostPicker.disabled = false;
  }
}

form.addEventListener('submit', (event) => {
  event.preventDefault();
  if (connectionBusy) return;
  clearLogin();
  act(form.querySelector('button'), () => connectRemote(link.value));
});
link.addEventListener('input', () => {
  if (!connectionBusy) clearLogin();
});

const sshForm = document.getElementById('ssh-form');
const sshFingerprint = document.getElementById('ssh-fingerprint');
const sshConnect = sshForm.querySelector('button[type="submit"]');
let pendingFingerprint;
sshForm.addEventListener('input', () => {
  pendingFingerprint = undefined;
  sshFingerprint.hidden = true;
  sshConnect.textContent = '连接 SSH';
});
sshForm.addEventListener('submit', (event) => {
  event.preventDefault();
  if (connectionBusy) return;
  act(sshConnect, async () => {
    const fields = new FormData(sshForm);
    const target = {
      host: fields.get('host'),
      port: Number(fields.get('port')),
      username: fields.get('username'),
      identityFile: fields.get('identityFile').trim() || undefined,
      executable: fields.get('executable').trim() || undefined,
    };
    connectionBusy = true;
    const inputs = [...sshForm.querySelectorAll('input')];
    for (const input of inputs) input.disabled = true;
    try {
      const result = await invoke('connect_ssh', {
        target,
        secret: fields.get('secret') || undefined,
        fingerprint: pendingFingerprint,
      });
      if (result.status === 'confirm_host') {
        pendingFingerprint = result.fingerprint;
        sshFingerprint.textContent =
          '请核对 SSH 主机指纹 ' + result.fingerprint;
        sshFingerprint.hidden = false;
        sshConnect.textContent = '信任并连接';
      } else {
        sshForm.elements.namedItem('secret').value = '';
        await close();
      }
    } finally {
      connectionBusy = false;
      for (const input of inputs) input.disabled = false;
    }
  });
});

window.wingmateLanHost = (id, host) => {
  if (id !== scanGeneration) return;
  const row = document.createElement('button');
  row.type = 'button';
  row.className = 'lan-host';
  row.setAttribute('aria-expanded', 'false');
  const address = new URL(host.origin).host;
  const text = document.createElement('span');
  if (host.name?.trim()) {
    const name = document.createElement('span');
    name.className = 'lan-host-name';
    name.textContent = host.name;
    text.append(name);
  }
  const endpoint = document.createElement('span');
  endpoint.className = 'muted';
  endpoint.textContent = address;
  text.append(endpoint);
  const arrow = document.createElement('span');
  arrow.className = 'lan-host-arrow';
  arrow.setAttribute('aria-hidden', 'true');
  arrow.textContent = '⌄';
  row.append(text, arrow);
  row.addEventListener('click', () => {
    if (connectionBusy) return;
    const toggle = row === selectedHost;
    clearLogin();
    if (!toggle) connectRemote(host.origin, undefined, row);
  });
  lanHosts.append(row);
};

lanSearch.addEventListener('click', () => {
  if (activeScan !== undefined) {
    const scan = activeScan;
    ++scanGeneration;
    activeScan = undefined;
    lanSearch.textContent = '搜索局域网';
    lanStatus.textContent = '已取消搜索';
    invoke('cancel_lan_scan', { scan }).catch(report);
    return;
  }
  if (connectionBusy) return;
  (async () => {
    clearLogin();
    const scan = ++scanGeneration;
    activeScan = scan;
    lanHosts.replaceChildren();
    lanStatus.textContent = '正在搜索…';
    lanSearch.textContent = '取消搜索';
    try {
      await recent;
      if (scan !== scanGeneration) return;
      const timedOut = await invoke('scan_lan', { scan });
      if (scan !== scanGeneration) return;
      if (timedOut === null) {
        lanStatus.textContent = '已取消搜索';
        return;
      }
      const count = lanHosts.querySelectorAll('.lan-host').length;
      lanStatus.textContent = timedOut
        ? count
          ? `搜索超时，已找到 ${count} 台主机`
          : '搜索超时，请确认在同一局域网且已开启分享。'
        : count
          ? `找到 ${count} 台主机`
          : '未发现主机，请确认在同一局域网且已开启分享。';
    } catch (error) {
      if (scan === scanGeneration) lanStatus.textContent = String(error);
    } finally {
      if (scan === scanGeneration) {
        activeScan = undefined;
        lanSearch.textContent = '搜索局域网';
      }
    }
  })();
});

const sharePanel = document.getElementById('share');
let sharePoll;
let shareRequest = 0;
let shareBusy = false;
let lanPairingId;
let lanPairingExpires;

async function updateShare(action = 'status') {
  clearTimeout(sharePoll);
  const request = ++shareRequest;
  shareBusy = action !== 'status';
  if (action === 'pair') renderPairings([]);
  try {
    const status = await invoke('local_share', { command: { action } });
    if (request !== shareRequest) return;
    localWebStatus.textContent = status.local_address
      ? `本机浏览器 · ${status.local_address}`
      : '本机 Web 入口不可用';
    localPair.hidden = !status.local_address;
    shareStatus.textContent =
      status.web_error ??
      (status.address ? `局域网分享 · ${status.address}` : '局域网分享已关闭');
    shareEnable.hidden = Boolean(status.address);
    sharePair.hidden = !status.address;
    if (
      lanPasswordForm.hidden ||
      status.lan_password_enabled !== lanPasswordEnabled
    )
      renderLanPassword(status.lan_password_enabled);
    if (action === 'pair') {
      lanPairingId = status.lan_pairing_id;
      lanPairingExpires = status.pairing_expires_at;
      renderPairings(status.pairings ?? []);
    } else if (
      selectedPairing &&
      (!status.address ||
        lanPairingExpires * 1000 <= Date.now() ||
        (lanPairingId && lanPairingId !== status.lan_pairing_id))
    ) {
      renderPairings([]);
    }
  } catch (error) {
    if (request === shareRequest) {
      if (action !== 'status') throw error;
      shareStatus.textContent = '无法读取分享状态';
      renderPairings([]);
      report(error);
    }
  } finally {
    if (request === shareRequest) {
      shareBusy = false;
      if (!sharePanel.hidden) sharePoll = setTimeout(updateShare, 1500);
    }
  }
}

const lanPasswordForm = document.getElementById('lan-password-form');
const lanPasswordInput = document.getElementById('local-lan-password');
const lanPasswordEdit = document.getElementById('lan-password-edit');
const lanPasswordSave = document.getElementById('lan-password-save');
const lanPasswordDisable = document.getElementById('lan-password-disable');
const lanPasswordNote = document.getElementById('lan-password-note');
const lanPasswordStatus = document.getElementById('lan-password-status');
let lanPasswordEnabled = false;
let editingLanPassword = false;
let revealGeneration = 0;
const showLanPassword = passwordEye(lanPasswordInput, (setVisible, button) => {
  const generation = ++revealGeneration;
  if (lanPasswordInput.type === 'text') {
    if (!editingLanPassword) lanPasswordInput.value = '';
    setVisible(false);
  } else if (editingLanPassword) setVisible(true);
  else
    act(button, async () => {
      const status = await invoke('local_share', {
        command: { action: 'lan_password_reveal' },
      });
      if (generation !== revealGeneration || editingLanPassword) return;
      lanPasswordInput.value = status.lan_password;
      setVisible(true);
    });
});

function renderLanPassword(enabled, editing = false) {
  ++revealGeneration;
  lanPasswordEnabled = Boolean(enabled);
  editingLanPassword = editing;
  const showField = enabled || editing;
  lanPasswordForm.hidden = false;
  lanPasswordInput.parentElement.hidden = !showField;
  lanPasswordInput.readOnly = !editing;
  lanPasswordInput.required = editing;
  lanPasswordInput.value = '';
  lanPasswordInput.placeholder = editing ? '至少 8 个字符' : '••••••••';
  showLanPassword(false);
  lanPasswordStatus.hidden = showField;
  lanPasswordEdit.textContent = enabled ? '修改' : '设置';
  lanPasswordEdit.hidden = editing;
  lanPasswordSave.hidden = !editing;
  lanPasswordNote.hidden = !editing;
  lanPasswordDisable.hidden = !enabled || editing;
  if (editing) lanPasswordInput.focus();
}

lanPasswordEdit.addEventListener('click', () =>
  renderLanPassword(lanPasswordEnabled, true),
);
document.getElementById('lan-password-cancel').addEventListener('click', () => {
  renderLanPassword(lanPasswordEnabled);
  lanPasswordEdit.focus();
});
lanPasswordForm.addEventListener('submit', (event) => {
  event.preventDefault();
  if (!editingLanPassword) return;
  act(lanPasswordSave, async () => {
    const status = await invoke('local_share', {
      command: {
        action: 'lan_password_set',
        password: lanPasswordInput.value,
        recoverable: true,
      },
    });
    renderLanPassword(status.lan_password_enabled);
    lanPasswordEdit.focus();
  });
});
lanPasswordDisable.addEventListener('click', () =>
  act(lanPasswordDisable, async () => {
    const status = await invoke('local_share', {
      command: { action: 'lan_password_disable' },
    });
    renderLanPassword(status.lan_password_enabled);
    lanPasswordEdit.focus();
  }),
);

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
  shareAddressTrigger.setAttribute(
    'aria-label',
    `切换配对地址，当前 ${pairing.address}`,
  );
  shareCode.alt = `${pairing.address} 的配对二维码`;
  shareCode.src = 'data:image/svg+xml,' + encodeURIComponent(pairing.qr);
  for (const option of shareAddressOptions.children) {
    option.setAttribute(
      'aria-pressed',
      String(option.textContent === pairing.address),
    );
  }
}

function renderPairings(entries) {
  const previousAddress = selectedPairing?.address;
  const pairings = entries.map((entry) => ({
    ...entry,
    address: new URL(entry.url).host,
  }));
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
  selectPairing(
    pairings.find((pairing) => pairing.address === previousAddress) ??
      pairings[0],
  );
}

shareAddressPicker.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape' || !shareAddressPicker.open) return;
  event.preventDefault();
  shareAddressPicker.open = false;
  shareAddressTrigger.focus();
});
shareAddressPicker.addEventListener('focusout', (event) => {
  if (!shareAddressPicker.contains(event.relatedTarget))
    shareAddressPicker.open = false;
});
document.addEventListener('pointerdown', (event) => {
  if (!shareAddressPicker.contains(event.target))
    shareAddressPicker.open = false;
});

function selectTab(tab) {
  for (const item of tabs) {
    const selected = item === tab;
    item.setAttribute('aria-selected', String(selected));
    item.tabIndex = selected ? 0 : -1;
    document.getElementById(item.getAttribute('aria-controls')).hidden =
      !selected;
  }
  message.textContent = '';
  clearTimeout(tunnelPoll);
  clearTimeout(sharePoll);
  if (tab.id === 'tunnel-tab' && !tunnelBusy) updateTunnel();
  if (tab.id !== 'share-tab' && !lanPasswordForm.hidden)
    renderLanPassword(lanPasswordEnabled);
  if (tab.id === 'share-tab' && !shareBusy) updateShare();
}

for (const tab of tabs) {
  tab.addEventListener('click', () => selectTab(tab));
  tab.addEventListener('keydown', (event) => {
    const target =
      event.key === 'Home'
        ? tabs[0]
        : event.key === 'End'
          ? tabs.at(-1)
          : event.key === 'ArrowLeft'
            ? tabs.at(tabs.indexOf(tab) - 1)
            : event.key === 'ArrowRight'
              ? tabs[(tabs.indexOf(tab) + 1) % tabs.length]
              : null;
    if (!target) return;
    event.preventDefault();
    selectTab(target);
    target.focus();
  });
}

shareEnable.addEventListener('click', () =>
  act(shareEnable, () => updateShare('enable')),
);
sharePair.addEventListener('click', () =>
  act(sharePair, () => updateShare('pair')),
);
localPair.addEventListener('click', () => {
  if (localPair.hasAttribute('aria-disabled')) return;
  act(localPair, async () => {
    const status = await invoke('local_share', {
      command: { action: 'pair_local' },
    });
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
const tunnelInvite = document.getElementById('tunnel-invite');
const tunnelDuration = document.getElementById('tunnel-duration');
const tunnelExpiry = document.getElementById('tunnel-expiry');
const tunnelPairExpiry = document.getElementById('tunnel-pair-expiry');
const tunnelRevoke = document.getElementById('tunnel-revoke');

let tunnel;
let tunnelMode;
let tunnelPairing;
let tunnelPoll;
let tunnelRequest = 0;
let tunnelBusy = false;
let tunnelReadError;
let tunnelInviteExpires;

function inviteTime(value) {
  return new Date(value * 1000).toLocaleString(undefined, {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function showTunnelPairing(pairing) {
  tunnelPairing = pairing;
  tunnelResult.hidden = !pairing;
  tunnelLink.hidden = true;
  tunnelLink.textContent = '';
  tunnelCopy.textContent = '复制链接';
  if (pairing) {
    tunnelCode.src = 'data:image/svg+xml,' + encodeURIComponent(pairing.qr);
    tunnelPairExpiry.textContent = pairing.named
      ? '可用一次'
      : `可用一次 · ${inviteTime(pairing.expires)} 到期`;
  } else tunnelCode.removeAttribute('src');
}

function renderTunnel() {
  const mode = tunnelMode ?? tunnel?.mode ?? 'quick';
  const running = Boolean(tunnel?.url);
  const progress = tunnel?.progress;
  const switching = running && tunnel.mode !== mode;
  const needsSettings = mode === 'named' && !tunnel?.hostname;
  const downloading = progress?.stage === 'download';
  const percent =
    downloading && progress.total
      ? Math.min(
          100,
          Math.floor(((progress.received ?? 0) * 100) / progress.total),
        )
      : null;
  const stages = {
    check: '正在检查 cloudflared…',
    download:
      percent === null
        ? '已下载 ' + ((progress?.received ?? 0) / 1048576).toFixed(1) + ' MiB'
        : '正在下载 cloudflared · ' + percent + '%',
    extract: '正在解压 cloudflared…',
    verify: '正在校验 cloudflared…',
    connect: '正在连接 Cloudflare…',
  };
  const text =
    tunnelReadError ??
    tunnel?.error ??
    (progress
      ? stages[progress.stage]
      : running
        ? '隧道已开启'
        : tunnel
          ? '隧道已关闭'
          : '请更新 Wingmate 管理服务');
  if (tunnelStatus.textContent !== text) tunnelStatus.textContent = text;
  tunnelProgress.hidden = !downloading;
  if (percent === null) tunnelProgress.removeAttribute('value');
  else {
    tunnelProgress.max = 100;
    tunnelProgress.value = percent;
  }
  tunnelAddress.textContent = tunnel?.url ?? '';
  tunnelAddress.title = tunnelAddress.textContent;
  tunnelAddress.hidden = !running;
  const originHint = tunnel?.named_service
    ? `Cloudflare Service URL 使用 ${tunnel.named_service}。`
    : '';
  tunnelHint.textContent = needsSettings
    ? '先在 Wingmate 后台服务详情页填写域名和 Tunnel Token。'
    : switching
      ? '切换会断开现有公网连接。'
      : mode === 'quick'
        ? '连接器重启后会分配新地址，需要重新配对。'
        : originHint;
  tunnelHint.hidden = !tunnelHint.textContent;
  tunnelStart.hidden =
    !tunnel ||
    Boolean(progress) ||
    needsSettings ||
    (running && !switching && mode === 'quick');
  tunnelStart.textContent = switching
    ? '切换并重启'
    : running
      ? '重启隧道'
      : '开启隧道';
  tunnelStop.hidden = !tunnel?.mode;
  const canPair = running && !switching;
  tunnelPair.hidden = !canPair;
  tunnelPair.textContent = tunnelPairing ? '重新生成' : '生成配对码';
  tunnelInvite.hidden = mode !== 'named' || !tunnelInviteExpires;
  tunnelDuration.hidden = !canPair || tunnel?.mode !== 'named';
  tunnelExpiry.textContent = tunnelInviteExpires
    ? `${inviteTime(tunnelInviteExpires)} 到期`
    : '';
  tunnelRevoke.hidden = !tunnelInviteExpires;
  tunnelSettings.hidden = !needsSettings || !tunnel;
  for (const button of tunnelModes) {
    button.setAttribute('aria-pressed', String(button.dataset.mode === mode));
    button.disabled = tunnelBusy || Boolean(progress);
  }
  for (const control of [
    tunnelStart,
    tunnelStop,
    tunnelPair,
    tunnelSettings,
    tunnelDuration,
    tunnelRevoke,
  ])
    control.disabled = tunnelBusy;
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
    tunnelInviteExpires = status.tunnel_invite_expires_at;
    if (command.action === 'tunnel_pair' && status.pairings?.[0]) {
      showTunnelPairing({
        ...status.pairings[0],
        id: status.tunnel_pairing_id,
        expires: status.pairing_expires_at,
        named: tunnel.mode === 'named',
      });
    } else if (
      tunnelPairing &&
      (tunnelPairing.expires * 1000 <= Date.now() ||
        (tunnelPairing.id && tunnelPairing.id !== status.tunnel_pairing_id) ||
        (tunnelPairing.named && tunnelInviteExpires !== tunnelPairing.expires))
    )
      showTunnelPairing(null);
  } catch (error) {
    if (id !== tunnelRequest) return;
    if (command.action === 'status') {
      tunnelReadError = String(error);
      showTunnelPairing(null);
    } else if (!tunnelPanel.hidden) report(error);
  } finally {
    if (id === tunnelRequest) {
      tunnelBusy = false;
      renderTunnel();
      if (!tunnelPanel.hidden)
        tunnelPoll = setTimeout(updateTunnel, tunnel?.progress ? 350 : 1500);
    }
  }
}

for (const button of tunnelModes)
  button.addEventListener('click', () => {
    tunnelMode = button.dataset.mode;
    renderTunnel();
  });
tunnelStart.addEventListener('click', () =>
  updateTunnel({
    action: 'tunnel_start',
    mode: tunnelMode ?? tunnel?.mode ?? 'quick',
  }),
);
tunnelStop.addEventListener('click', () =>
  updateTunnel({ action: 'tunnel_stop' }),
);
tunnelPair.addEventListener('click', () =>
  updateTunnel({
    action: 'tunnel_pair',
    long_lived: tunnel.mode === 'named' && tunnelDuration.value === '24h',
  }),
);
tunnelRevoke.addEventListener('click', () =>
  updateTunnel({ action: 'tunnel_revoke' }),
);
tunnelSettings.addEventListener('click', () =>
  act(tunnelSettings, () => invoke('open_tunnel_settings')),
);
tunnelCopy.addEventListener('click', () =>
  act(tunnelCopy, async () => {
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
  }),
);

if (passwordOnly) selectTab(document.getElementById('share-tab'));
