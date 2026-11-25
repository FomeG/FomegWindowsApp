import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const host = process.env.TAURI_DEV_HOST;

// Cấu hình kiểu Tauri: cổng cố định 1420, không xoá màn hình console.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    // chỉ theo dõi giao diện: target/ (build Rust ở gốc workspace, hàng chục nghìn tệp, exe/pdb bị khoá lúc
    // build -> "EBUSY") và các thư mục không phải giao diện không cần theo dõi
    watch: {
      ignored: ['**/src-tauri/**', '**/target/**', '**/mcp/**', '**/RustWTM/**', '**/docs/**', '**/codex/**', '**/screenshots/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    outDir: 'dist',
    sourcemap: false,
    chunkSizeWarningLimit: 600,
  },
});
