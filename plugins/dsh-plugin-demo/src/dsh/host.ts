import type { IncomingMessage, ServerResponse } from 'node:http';
import { handleStatistics } from '../server/statistics-service.ts';
import { statisticsPath } from '../types.ts';

export const name = 'dsh-plugin-demo';
export const inject = ['webServer'];
type Context = {
  effect(install: () => () => void): void;
  webServer: {
    register(route: {
      kind: 'exact';
      path: string;
      handler(req: IncomingMessage, res: ServerResponse): Promise<void>;
    }): () => void;
  };
};

export function apply(ctx: Context) {
  ctx.effect(() =>
    ctx.webServer.register({
      kind: 'exact',
      path: statisticsPath,
      handler: handleStatistics,
    }),
  );
}
