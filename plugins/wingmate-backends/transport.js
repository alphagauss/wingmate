(() => {
  const backend = new URL(location.href).searchParams.get('wingmate-agent');
  // DSH's navigation store has one unscoped persistence key. Keep each
  // page's backend selections independent, including after a full reload.
  const selection = 'dsh.sessions.current';
  const scopedSelection = `wingmate.current.${backend ?? 'dsh'}`;
  const getItem = Storage.prototype.getItem;
  const setItem = Storage.prototype.setItem;
  Storage.prototype.getItem = function (key) {
    if (this === localStorage && key === selection) {
      return getItem.call(sessionStorage, scopedSelection);
    }
    return getItem.call(this, key);
  };
  Storage.prototype.setItem = function (key, value) {
    if (this === localStorage && key === selection) {
      return setItem.call(sessionStorage, scopedSelection, value);
    }
    return setItem.call(this, key, value);
  };
  if (!globalThis.__WINGMATE_AGENTS__?.some(agent => agent.id === backend)) return;
  const original = globalThis.__DSH_TRANSPORT__;
  const request = original?.fetch ?? ((input, options) => fetch(input, options));
  const connect = original?.createWebSocket ?? (url => new WebSocket(url));

  globalThis.__DSH_TRANSPORT__ = {
    ...original,
    fetch(input, options) {
      const message = JSON.parse(options.body);
      message.payload = { ...message.payload, wingmateAgent: backend };
      return request(input, { ...options, body: JSON.stringify(message) }).then(async response => {
        if (message.method === 'session/list') {
          const { result } = await response.clone().json();
          globalThis.__WINGMATE_AGENT_ERROR__ = result.ok ? '' : result.error.message;
          dispatchEvent(new Event('wingmate:agent-status'));
        }
        return response;
      });
    },
    createWebSocket(url) {
      const socket = connect(url);
      if (!new URL(url).pathname.endsWith('/api/remote.mux')) return socket;
      const send = socket.send.bind(socket);
      socket.send = data => {
        const message = JSON.parse(data);
        if (message.type === 'open') {
          message.payload = { ...message.payload, wingmateAgent: backend };
          return send(JSON.stringify(message));
        }
        return send(data);
      };
      return socket;
    },
  };
})();
