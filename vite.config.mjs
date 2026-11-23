import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [tailwindcss(), react()],

  // Vitest's default exclude is only node_modules and .git; agents' worktrees live under .claude/.
  test: {
    exclude: ['**/node_modules/**', '**/.git/**', '**/.claude/**'],
  },

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**', '**/.claude/**'], // Argentum: agents' worktrees live under .claude/
    },
    warmup: { clientFiles: ['./src/main.tsx'] }, // Argentum: transform the app while cargo builds
  },

  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));
