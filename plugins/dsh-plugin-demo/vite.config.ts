import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import { handleStatistics } from './src/server/statistics-service.ts';
import { statisticsPath } from './src/types.ts';

export default defineConfig({
  root: fileURLToPath(new URL('./src/web', import.meta.url)),
  plugins: [
    react(),
    {
      name: 'demo-api',
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          if (
            new URL(req.url || '/', 'http://localhost').pathname ===
            statisticsPath
          ) {
            void handleStatistics(req, res);
          } else next();
        });
      },
    },
  ],
  build: {
    outDir: fileURLToPath(new URL('./dist/web', import.meta.url)),
    emptyOutDir: true,
  },
});
