export const diagnosticsPath = '/api/wingmate-backends';

export type BackendStatus = {
  id: string;
  label: string;
  adapterAvailable: boolean;
  connection: 'unchecked' | 'connected' | 'failed' | 'disconnected';
  detail?: string;
};
type Backend = {
  diagnostics(): BackendStatus;
  checkConnection(): Promise<void>;
};
type HostContext = { connection: { fetch: { register(route: {
  path: string;
  methods: readonly ('GET' | 'POST')[];
  requestBody: 'buffered';
  fetch(request: Request): Promise<Response>;
}): () => Promise<void> } } };

export function installDiagnostics(ctx: HostContext, backends: ReadonlyMap<string, Backend>) {
  // Exact routes under /api are authenticated by DSH and only accept owner Peers.
  ctx.connection.fetch.register({
    path: diagnosticsPath,
    methods: ['GET', 'POST'],
    requestBody: 'buffered',
    async fetch(request) {
      if (request.method === 'POST') {
        if (request.headers.get('content-type') !== 'application/json') return new Response(null, { status: 415 });
        const payload = await request.json().catch(() => undefined) as { id?: unknown } | undefined;
        const backend = typeof payload?.id === 'string' ? backends.get(payload.id) : undefined;
        if (!backend) return new Response(null, { status: 400 });
        request.signal.throwIfAborted();
        await backend.checkConnection();
      }
      return Response.json([...backends.values()].map(backend => backend.diagnostics()), {
        headers: { 'Cache-Control': 'no-store' },
      });
    },
  });
}
