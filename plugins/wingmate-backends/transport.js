(() => {
  const backend = new URL(location.href).searchParams.get('wingmate-agent');
  if (!globalThis.__WINGMATE_AGENTS__?.some(agent => agent.id === backend)) return;
  const original = globalThis.__DSH_TRANSPORT__;
  const request = original?.fetch ?? ((input, options) => fetch(input, options));
  const connect = original?.createWebSocket ?? (url => new WebSocket(url));

  globalThis.__DSH_TRANSPORT__ = {
    ...original,
    fetch(input, options) {
      const message = JSON.parse(options.body);
      message.payload = { ...message.payload, wingmateAgent: backend };
      return request(input, { ...options, body: JSON.stringify(message) });
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
