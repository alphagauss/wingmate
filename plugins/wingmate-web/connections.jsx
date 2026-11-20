import { detailHeading, detailSection } from './preinstalled.tsx';

const tabLabels = {
  share: '分享此主机',
  connect: '连接主机',
  devices: '已配对设备',
};
const panels = { display: 'grid', gap: 16 };
const section = {
  ...detailSection,
  borderTop: '0.5px solid var(--dsw-alias-border-l2)',
  paddingTop: 14,
};
const inset = {
  ...detailSection,
  padding: '12px 16px',
  background: 'var(--dsw-alias-bg-module-platform)',
  borderRadius: 'var(--dsw-radius-md)',
};
const selectStyle = {
  minHeight: 28,
  maxWidth: '100%',
  border: '1px solid var(--dsw-alias-border-l2)',
  borderRadius: 'var(--dsw-radius-sm)',
  background: 'var(--dsw-alias-bg-layer-3)',
  color: 'var(--dsw-alias-label-primary)',
  font: 'inherit',
  fontSize: 13,
  padding: '0 6px',
};
const stages = {
  check: '正在检查 cloudflared…',
  extract: '正在解压 cloudflared…',
  verify: '正在校验 cloudflared…',
  connect: '正在连接 Cloudflare…',
};

let scanId = 0;
let requestTunnelSettings = false;

export function revealTunnelSettings() {
  requestTunnelSettings = true;
  window.dispatchEvent(new Event('wingmate:show-named-tunnel'));
}

function inviteTime(expires) {
  return new Date(expires * 1000).toLocaleString(undefined, {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function currentPairing(pairing, id) {
  return pairing &&
    pairing.expires * 1000 > Date.now() &&
    (!pairing.id || pairing.id === id)
    ? pairing
    : null;
}

export function createConnectionDetails(react, primitives, SettingRow, styles) {
  const { createElement: h, useEffect, useId, useRef, useState } = react;
  const { Button, Input, Modal, SegmentedTabs, Switch } = primitives;
  const { actionsStyle, alertStyle, detailStyle, valueStyle } = styles;

  function Copy({ value, children = '复制链接' }) {
    const [message, setMessage] = useState('');
    useEffect(() => setMessage(''), [value]);
    return (
      <div style={detailSection}>
        <div style={actionsStyle}>
          <Button
            size="sm"
            variant="outline"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(value);
                setMessage('已复制');
              } catch {
                setMessage('请手动复制链接');
              }
            }}
          >
            {children}
          </Button>
          <span style={detailStyle} role="status">
            {message}
          </span>
        </div>
        {message === '请手动复制链接' && (
          <code style={valueStyle}>{value}</code>
        )}
      </div>
    );
  }

  function Pairing({ pairing }) {
    const [address, setAddress] = useState('');
    const entries = pairing.entries;
    const entry =
      entries.find((item) => new URL(item.url).origin === address) ??
      entries[0];
    return (
      <div
        style={{
          ...inset,
          gridTemplateColumns:
            'repeat(auto-fit, minmax(min(100%, 140px), 1fr))',
          alignItems: 'center',
        }}
      >
        <img
          width={116}
          height={116}
          style={{ background: 'white' }}
          alt="配对二维码"
          src={'data:image/svg+xml,' + encodeURIComponent(entry.qr)}
        />
        <div style={{ ...detailSection, minWidth: 0 }}>
          {entries.length > 1 ? (
            <select
              aria-label="配对地址"
              style={selectStyle}
              value={new URL(entry.url).origin}
              onChange={(event) => setAddress(event.target.value)}
            >
              {entries.map((item) => {
                const origin = new URL(item.url).origin;
                return (
                  <option key={origin} value={origin}>
                    {origin}
                  </option>
                );
              })}
            </select>
          ) : (
            <code style={valueStyle}>{new URL(entry.url).origin}</code>
          )}
          <span style={detailStyle}>
            可用一次 · {inviteTime(pairing.expires)} 到期
          </span>
          <Copy value={entry.url} />
        </div>
      </div>
    );
  }

  function NamedTunnel({ tunnel, expanded, pending, run }) {
    const details = useRef(null);
    const [notice, setNotice] = useState('');
    useEffect(() => {
      if (expanded || !tunnel.hostname) details.current.open = true;
    }, [expanded, tunnel.hostname]);

    async function save(event) {
      event.preventDefault();
      const form = event.currentTarget;
      const data = new FormData(form);
      setNotice('');
      if (
        await run({
          action: 'tunnel_configure',
          hostname: data.get('hostname'),
          token: data.get('token')?.trim() || undefined,
        })
      ) {
        form.elements.namedItem('token').value = '';
        setNotice('已保存，下次开启或重启隧道时生效。');
      }
    }

    return (
      <details ref={details} style={inset}>
        <summary style={{ cursor: 'pointer' }}>
          具名隧道配置{' '}
          <span style={detailStyle}>
            {tunnel.hostname ? '已配置' : '未配置'}
          </span>
        </summary>
        <form
          method="post"
          style={{ ...detailSection, marginTop: 12, maxWidth: 420 }}
          onSubmit={save}
        >
          <label style={detailSection}>
            公开域名
            <Input
              name="hostname"
              key={tunnel.hostname}
              defaultValue={tunnel.hostname ?? ''}
              required
              placeholder="wingmate.example.com"
              spellCheck={false}
              autoComplete="off"
              disabled={pending}
            />
          </label>
          <label style={detailSection}>
            Tunnel Token
            <Input
              name="token"
              type="password"
              autoComplete="off"
              disabled={pending}
              required={!tunnel.hostname}
              placeholder={
                tunnel.hostname
                  ? '留空保留已保存的 Token'
                  : '粘贴 Cloudflare Tunnel Token'
              }
            />
          </label>
          <div style={detailSection}>
            <span style={detailStyle}>Cloudflare 发布路由的 Service URL</span>
            <code style={valueStyle}>
              {tunnel.named_service ?? '本机 Web 入口不可用'}
            </code>
            {tunnel.named_service && (
              <Copy value={tunnel.named_service}>复制地址</Copy>
            )}
          </div>
          <div style={actionsStyle}>
            <Button
              type="submit"
              size="sm"
              variant="outline"
              disabled={pending}
            >
              保存配置
            </Button>
            <span role="status" style={detailStyle}>
              {notice}
            </span>
          </div>
        </form>
      </details>
    );
  }

  function ShareHost({
    status,
    lanPair,
    publicPair,
    mode,
    setMode,
    expanded,
    bridge,
    pending,
    run,
    confirm,
    report,
  }) {
    const [localNotice, setLocalNotice] = useState('');
    const [localLink, setLocalLink] = useState('');
    const [duration, setDuration] = useState('5m');
    const tunnel = status.tunnel;
    const selectedMode = mode ?? tunnel?.mode ?? 'quick';
    const named = selectedMode === 'named';
    const running = Boolean(tunnel?.url);
    const switching = running && tunnel.mode !== selectedMode;
    const progress = tunnel?.progress;
    const percent = progress?.total
      ? Math.min(100, Math.floor((progress.received * 100) / progress.total))
      : undefined;
    const tunnelStatus =
      tunnel?.error ??
      (progress?.stage === 'download'
        ? percent === undefined
          ? '正在下载 cloudflared…'
          : `正在下载 cloudflared · ${percent}%`
        : (stages[progress?.stage] ??
          (running
            ? `运行中 · ${tunnel.mode === 'named' ? '具名隧道' : '临时隧道'}`
            : '已关闭')));
    const startLabel = switching
      ? '切换并重启'
      : running
        ? '重启隧道'
        : '开启隧道';
    const start = { action: 'tunnel_start', mode: selectedMode };
    const addresses = status.lan_addresses?.length
      ? status.lan_addresses
      : [status.address];

    async function localPair() {
      setLocalNotice('');
      setLocalLink('');
      const reply = await run({ action: 'pair_local' });
      if (!reply?.pairing_url) return;
      try {
        await navigator.clipboard.writeText(reply.pairing_url);
        setLocalNotice('已复制 · 5 分钟内可用一次');
      } catch {
        setLocalLink(reply.pairing_url);
        setLocalNotice('请手动复制 · 5 分钟内可用一次');
      }
    }

    return (
      <div style={panels}>
        <section aria-label="本机浏览器">
          <SettingRow
            label="本机浏览器"
            last
            compact
            control={
              <Button
                size="sm"
                variant="outline"
                aria-label="复制本机配对链接"
                disabled={pending || !status.local_address}
                onClick={localPair}
              >
                复制配对链接
              </Button>
            }
          >
            <code style={valueStyle}>
              {status.local_address
                ? `http://${status.local_address}`
                : '本机 Web 入口不可用'}
            </code>
            {localNotice && (
              <span style={detailStyle} role="status">
                {localNotice}
              </span>
            )}
            {localLink && <code style={valueStyle}>{localLink}</code>}
          </SettingRow>
        </section>
        <section aria-label="局域网分享" style={section}>
          <SettingRow
            label="局域网分享"
            last
            compact
            control={
              <Switch
                label="允许局域网访问"
                checked={Boolean(status.address)}
                disabled={pending}
                onChange={(enabled) =>
                  enabled
                    ? run({ action: 'enable', address: null })
                    : confirm('关闭局域网分享', '当前局域网连接会断开。', {
                        action: 'disable',
                      })
                }
              />
            }
          >
            <span style={detailStyle}>通过 HTTP 连接，适用于可信网络。</span>
          </SettingRow>
          <div style={{ display: 'grid', gap: 10, paddingInlineStart: 12 }}>
            <SettingRow
              label="局域网密码"
              last
              compact
              control={
                bridge.openLanPassword && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => bridge.openLanPassword().catch(report)}
                  >
                    {status.lan_password_enabled ? '管理密码' : '设置密码'}
                  </Button>
                )
              }
            >
              <span style={detailStyle}>
                {status.lan_password_enabled ? '已设置' : '未设置'}
                {!bridge.openLanPassword && ' · 在标题栏的远程连接浮窗中管理'}
              </span>
            </SettingRow>
            {status.address && (
              <SettingRow
                label="局域网配对"
                last
                compact
                control={
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={pending}
                    onClick={() => run({ action: 'pair' })}
                  >
                    {lanPair ? '重新生成' : '生成配对链接'}
                  </Button>
                }
              >
                {addresses.map((address) => (
                  <code key={address} style={valueStyle}>
                    http://{address}
                  </code>
                ))}
              </SettingRow>
            )}
          </div>
          {lanPair && <Pairing pairing={lanPair} />}
        </section>
        <section aria-label="公网隧道" style={section}>
          <SettingRow
            label="公网隧道"
            last
            compact
            wrap
            control={
              tunnel && (
                <div style={actionsStyle}>
                  <select
                    aria-label="隧道模式"
                    style={selectStyle}
                    value={selectedMode}
                    disabled={pending || Boolean(progress)}
                    onChange={(event) => setMode(event.target.value)}
                  >
                    <option value="quick">临时隧道</option>
                    <option value="named">具名隧道</option>
                  </select>
                  {!progress &&
                    !(named && !tunnel.hostname) &&
                    !(running && !switching && !named) && (
                      <Button
                        size="sm"
                        variant={running ? 'outline' : 'primary'}
                        disabled={pending}
                        onClick={() =>
                          running
                            ? confirm(startLabel, '当前公网连接会断开。', start)
                            : run(start)
                        }
                      >
                        {startLabel}
                      </Button>
                    )}
                  {tunnel.mode && (
                    <Button
                      size="sm"
                      disabled={pending}
                      onClick={() => {
                        setMode(selectedMode);
                        confirm('关闭隧道', '当前公网连接会断开。', {
                          action: 'tunnel_stop',
                        });
                      }}
                    >
                      关闭隧道
                    </Button>
                  )}
                </div>
              )
            }
          >
            <span style={detailStyle} role="status">
              {tunnel ? tunnelStatus : '请更新 Wingmate 管理服务'}
            </span>
            {tunnel?.url && <code style={valueStyle}>{tunnel.url}</code>}
          </SettingRow>
          {progress?.stage === 'download' && (
            <progress aria-label="连接器下载进度" max={100} value={percent} />
          )}
          {tunnel && (
            <>
              {named ? (
                <NamedTunnel
                  tunnel={tunnel}
                  pending={pending}
                  run={run}
                  expanded={expanded}
                />
              ) : (
                running &&
                !switching && (
                  <span style={detailStyle}>
                    临时隧道重启后会分配新地址，需要重新配对。
                  </span>
                )
              )}
              {((running && !switching) ||
                (named && status.tunnel_invite_expires_at)) && (
                <SettingRow
                  label="公网配对"
                  compact
                  last
                  wrap
                  control={
                    <div style={actionsStyle}>
                      {running && !switching && (
                        <>
                          {named && (
                            <select
                              aria-label="邀请有效期"
                              style={selectStyle}
                              value={duration}
                              disabled={pending}
                              onChange={(event) =>
                                setDuration(event.target.value)
                              }
                            >
                              <option value="5m">5 分钟</option>
                              <option value="24h">24 小时</option>
                            </select>
                          )}
                          <Button
                            size="sm"
                            variant="primary"
                            disabled={pending}
                            onClick={() =>
                              run({
                                action: 'tunnel_pair',
                                long_lived: named && duration === '24h',
                              })
                            }
                          >
                            {publicPair ||
                            (named && status.tunnel_invite_expires_at)
                              ? '重新生成'
                              : '生成配对链接'}
                          </Button>
                        </>
                      )}
                      {named && status.tunnel_invite_expires_at && (
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={pending}
                          onClick={() => run({ action: 'tunnel_revoke' })}
                        >
                          撤销邀请
                        </Button>
                      )}
                    </div>
                  }
                >
                  <span style={detailStyle}>
                    {named && status.tunnel_invite_expires_at
                      ? `未使用的邀请 · ${inviteTime(status.tunnel_invite_expires_at)} 到期`
                      : `可用一次 · ${named && duration === '24h' ? '24 小时' : '5 分钟'}有效`}
                  </span>
                </SettingRow>
              )}
              {!switching && publicPair && <Pairing pairing={publicPair} />}
            </>
          )}
        </section>
        {status.web_error && (
          <div role="alert" style={alertStyle}>
            {status.web_error}
          </div>
        )}
      </div>
    );
  }

  function ConnectHost({ bridge }) {
    const [history, setHistory] = useState([]);
    const [address, setAddress] = useState('');
    const [hosts, setHosts] = useState([]);
    const [searching, setSearching] = useState(false);
    const [scanStatus, setScanStatus] = useState('');
    const [busy, setBusy] = useState(false);
    const [auth, setAuth] = useState(null);
    const [showPassword, setShowPassword] = useState(false);
    const [error, setError] = useState(null);
    const active = useRef(null);
    const addressInput = useRef(null);

    useEffect(() => {
      let mounted = true;
      const refresh = () =>
        bridge
          .connectionHistory()
          .then((items) => {
            if (mounted) setHistory(items);
          })
          .catch((reason) => {
            if (mounted) setError({ row: '', message: String(reason) });
          });
      refresh();
      window.addEventListener('focus', refresh);
      const reportHost = (id, host) => {
        if (active.current?.id !== id) return;
        active.current.count += 1;
        setHosts((items) => [...items, host]);
      };
      window.wingmateLanHost = reportHost;
      return () => {
        mounted = false;
        window.removeEventListener('focus', refresh);
        delete window.wingmateLanHost;
        if (active.current) {
          bridge.cancelLanScan(active.current.id).catch(() => {});
          active.current = null;
        }
      };
    }, [bridge]);

    function stopSearch() {
      if (!active.current) return;
      const { id } = active.current;
      active.current = null;
      setSearching(false);
      setScanStatus('已取消搜索');
      bridge.cancelLanScan(id).catch((reason) => setScanStatus(String(reason)));
    }

    async function search() {
      if (active.current) return stopSearch();
      const scan = { id: ++scanId, count: 0 };
      active.current = scan;
      setHosts([]);
      setAuth(null);
      setError(null);
      setSearching(true);
      setScanStatus('正在搜索…');
      try {
        const timedOut = await bridge.scanLan(scan.id);
        if (active.current !== scan) return;
        if (timedOut === null) {
          setScanStatus('已取消搜索');
          return;
        }
        const result = scan.count
          ? `找到 ${scan.count} 台主机`
          : '未发现主机，请确认在同一局域网且已开启分享。';
        setScanStatus(timedOut ? `搜索超时 · ${result}` : result);
      } catch (reason) {
        if (active.current === scan) setScanStatus(String(reason));
      } finally {
        if (active.current === scan) {
          active.current = null;
          setSearching(false);
        }
      }
    }

    async function connect(link, row, password, requested = link) {
      setBusy(true);
      setError(null);
      if (password === undefined) {
        setAuth(null);
        setShowPassword(false);
      }
      try {
        const result = await bridge.connectRemote(link, password, requested);
        if (result.needs_password || result.needs_pairing) {
          setAuth({ ...result, row, requested });
        } else {
          setAddress('');
          setAuth(null);
          stopSearch();
          bridge
            .connectionHistory()
            .then(setHistory)
            .catch(() => {});
        }
      } catch (reason) {
        setError({ row, message: String(reason) });
      } finally {
        setBusy(false);
      }
    }

    function authFor(row) {
      return (
        <>
          {auth?.row === row &&
            (auth.needs_pairing ? (
              <div style={actionsStyle}>
                <span style={detailStyle}>请从主机获取配对链接。</span>
                <Button size="sm" onClick={() => addressInput.current?.focus()}>
                  粘贴配对链接
                </Button>
              </div>
            ) : (
              <form
                method="post"
                style={inset}
                onSubmit={(event) => {
                  event.preventDefault();
                  const password =
                    event.currentTarget.elements.namedItem('password');
                  connect(auth.origin, row, password.value, auth.requested);
                  password.value = '';
                }}
              >
                <div style={actionsStyle}>
                  <Input
                    name="password"
                    type={showPassword ? 'text' : 'password'}
                    aria-label="局域网密码"
                    placeholder="局域网密码"
                    autoComplete="off"
                    autoFocus
                    required
                    maxLength={256}
                    disabled={busy}
                  />
                  <Button
                    size="sm"
                    aria-pressed={showPassword}
                    onClick={() => setShowPassword(!showPassword)}
                  >
                    {showPassword ? '隐藏密码' : '显示密码'}
                  </Button>
                  <Button
                    size="sm"
                    type="submit"
                    variant="primary"
                    disabled={busy}
                  >
                    连接
                  </Button>
                </div>
              </form>
            ))}
          {error?.row === row && (
            <div style={alertStyle} role="alert">
              {error.message}
            </div>
          )}
        </>
      );
    }

    function hostRow(origin, name, row) {
      return (
        <div key={row}>
          <SettingRow
            label={
              <span style={{ overflowWrap: 'anywhere' }}>{name || origin}</span>
            }
            compact
            control={
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                aria-expanded={auth?.row === row}
                onClick={() => {
                  if (auth?.row === row) {
                    setAuth(null);
                    setError(null);
                  } else {
                    connect(origin, row);
                  }
                }}
              >
                连接
              </Button>
            }
          >
            {name && <code style={valueStyle}>{origin}</code>}
          </SettingRow>
          {authFor(row)}
        </div>
      );
    }

    return (
      <div style={panels}>
        <form
          method="post"
          style={detailSection}
          onSubmit={(event) => {
            event.preventDefault();
            connect(address, '');
          }}
        >
          <h2 style={detailHeading}>连接地址或配对链接</h2>
          <div style={actionsStyle}>
            <div style={{ flex: '1 1 220px', minWidth: 0 }}>
              <Input
                ref={addressInput}
                aria-label="连接地址或配对链接"
                type="url"
                required
                autoComplete="off"
                placeholder="http://192.168.1.10:8746 或 https://域名/"
                value={address}
                disabled={busy}
                onChange={(event) => {
                  setAddress(event.target.value);
                  setAuth(null);
                  setError(null);
                }}
              />
            </div>
            <Button type="submit" variant="primary" disabled={busy}>
              连接
            </Button>
          </div>
          {busy && (
            <span style={detailStyle} role="status">
              正在连接…
            </span>
          )}
        </form>
        {authFor('')}
        <section aria-label="最近连接" style={detailSection}>
          <h2 style={detailHeading}>最近连接</h2>
          {history.length ? (
            history.map((origin) => hostRow(origin, null, `recent:${origin}`))
          ) : (
            <span style={detailStyle}>暂无最近连接</span>
          )}
        </section>
        <section aria-label="局域网主机" style={detailSection}>
          <SettingRow
            label="局域网主机"
            compact
            last
            control={
              <Button
                size="sm"
                variant="outline"
                disabled={busy && !searching}
                onClick={search}
              >
                {searching ? '取消搜索' : '搜索局域网'}
              </Button>
            }
          >
            <span style={detailStyle} role="status">
              {scanStatus || '搜索同一网络中的 Wingmate 主机。'}
            </span>
          </SettingRow>
          {hosts.map((host) =>
            hostRow(host.origin, host.name, `scan:${host.origin}`),
          )}
        </section>
      </div>
    );
  }

  return function ConnectionDetails({ bridge }) {
    const baseId = useId();
    const [tab, setTab] = useState('share');
    const [status, setStatus] = useState(null);
    const [pending, setPending] = useState(false);
    const [error, setError] = useState('');
    const [statusError, setStatusError] = useState('');
    const [lanPair, setLanPair] = useState(null);
    const [publicPair, setPublicPair] = useState(null);
    const [mode, setMode] = useState();
    const [expanded, setExpanded] = useState(0);
    const [confirmation, setConfirmation] = useState(null);
    const updating = useRef(false);
    const sequence = useRef(0);
    const report = (reason) => setError(String(reason));
    const confirm = (label, description, command) =>
      setConfirmation({ label, description, command });

    async function run(command = { action: 'status' }) {
      if (updating.current) return;
      const id = ++sequence.current;
      const mutation = command.action !== 'status';
      if (mutation) {
        updating.current = true;
        setPending(true);
        setError('');
        if (command.action === 'pair') setLanPair(null);
        if (command.action === 'tunnel_pair') setPublicPair(null);
      }
      try {
        const reply = await bridge.share(command);
        if (id !== sequence.current) return;
        setStatus(reply);
        setStatusError('');
        const pair = (id, entries) => ({
          id,
          entries,
          expires: reply.pairing_expires_at,
        });
        setLanPair((current) =>
          command.action === 'pair' && reply.pairings?.length
            ? pair(reply.lan_pairing_id, reply.pairings)
            : reply.address
              ? currentPairing(current, reply.lan_pairing_id)
              : null,
        );
        setPublicPair((current) =>
          command.action === 'tunnel_pair' && reply.pairings?.length
            ? {
                ...pair(reply.tunnel_pairing_id, reply.pairings),
                address: reply.tunnel.url,
                named: reply.tunnel.mode === 'named',
              }
            : current?.address === reply.tunnel?.url &&
                (!current?.named ||
                  current.expires === reply.tunnel_invite_expires_at)
              ? currentPairing(current, reply.tunnel_pairing_id)
              : null,
        );
        return reply;
      } catch (reason) {
        if (id === sequence.current) {
          if (mutation) report(reason);
          else {
            setStatusError(String(reason));
            setLanPair(null);
            setPublicPair(null);
          }
        }
      } finally {
        if (mutation) {
          updating.current = false;
          setPending(false);
        }
      }
    }

    useEffect(() => {
      let disposed = false;
      let timer;
      async function poll() {
        const reply = await run();
        if (!disposed)
          timer = setTimeout(poll, reply?.tunnel?.progress ? 350 : 1500);
      }
      if (tab !== 'connect') poll();
      return () => {
        disposed = true;
        clearTimeout(timer);
      };
    }, [bridge, tab]);

    useEffect(() => {
      function openTunnel() {
        if (!requestTunnelSettings) return;
        requestTunnelSettings = false;
        setTab('share');
        setMode('named');
        setExpanded((value) => value + 1);
      }
      openTunnel();
      window.addEventListener('wingmate:show-named-tunnel', openTunnel);
      return () =>
        window.removeEventListener('wingmate:show-named-tunnel', openTunnel);
    }, []);

    const tabs = Object.entries(tabLabels)
      .filter(([value]) => value !== 'connect' || bridge.connectRemote)
      .map(([value, label]) => ({
        value,
        label,
        id: `${baseId}-${value}`,
        panelId: `${baseId}-${value}-panel`,
      }));
    return (
      <div style={detailSection}>
        <div style={{ width: 'min(100%, 390px)', marginBottom: 4 }}>
          <SegmentedTabs
            items={tabs}
            value={tab}
            onChange={(value) => {
              setError('');
              setTab(value);
            }}
            label="后台服务功能"
            size="sm"
          />
        </div>
        {tabs.map(({ value, id, panelId }) => (
          <div
            key={value}
            id={panelId}
            role="tabpanel"
            aria-labelledby={id}
            hidden={tab !== value}
          >
            {value === 'share' && status && (
              <ShareHost
                {...{
                  status,
                  lanPair,
                  publicPair,
                  mode,
                  setMode,
                  expanded,
                  bridge,
                  pending,
                  run,
                  confirm,
                  report,
                }}
              />
            )}
            {value === 'connect' && tab === value && (
              <ConnectHost bridge={bridge} />
            )}
            {value === 'devices' && status && (
              <section aria-label="已配对设备" style={detailSection}>
                <SettingRow
                  label="已配对设备"
                  compact
                  last
                  control={
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={pending}
                      onClick={() => run()}
                    >
                      刷新
                    </Button>
                  }
                />
                {status.devices.length === 0 && (
                  <span style={detailStyle}>暂无已配对设备</span>
                )}
                {status.devices.map((device, index) => (
                  <SettingRow
                    key={device.id}
                    label={device.id.slice(0, 8)}
                    last={index === status.devices.length - 1}
                    control={
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={pending}
                        onClick={() =>
                          confirm(
                            '撤销设备',
                            '该设备的连接会断开，再次访问时需重新配对。',
                            { action: 'revoke', id: device.id },
                          )
                        }
                      >
                        撤销
                      </Button>
                    }
                  >
                    <span style={detailStyle}>
                      {
                        { local: '本机', lan: '局域网', public: '公网' }[
                          device.access
                        ]
                      }{' '}
                      · {new Date(device.paired_at * 1000).toLocaleString()}
                    </span>
                  </SettingRow>
                ))}
              </section>
            )}
          </div>
        ))}
        {tab !== 'connect' && !status && !statusError && (
          <span style={detailStyle} role="status">
            正在读取分享状态…
          </span>
        )}
        {tab !== 'connect' && statusError && (
          <span style={alertStyle} role="alert">
            {statusError}
          </span>
        )}
        {error && (
          <span style={alertStyle} role="alert">
            {error}
          </span>
        )}
        <Modal
          open={Boolean(confirmation)}
          onClose={() => setConfirmation(null)}
          title={confirmation ? `${confirmation.label}？` : ''}
          description={confirmation?.description}
          closeLabel="关闭确认"
          footer={
            <div style={actionsStyle}>
              <Button
                data-modal-autofocus
                onClick={() => setConfirmation(null)}
              >
                取消
              </Button>
              <Button
                variant="primary"
                onClick={() => {
                  if (confirmation) run(confirmation.command);
                  setConfirmation(null);
                }}
              >
                {confirmation?.label}
              </Button>
            </div>
          }
        />
      </div>
    );
  };
}
