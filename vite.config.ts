import type { ProxyOptions } from "vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  root: "web",
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    proxy: Object.fromEntries(["/api", "/session", "/events", "/downloads"].map(path => [path, { target: "http://127.0.0.1:8765", changeOrigin: true, configure(proxy) { proxy.on("proxyReq", request => request.setHeader("Origin", "http://127.0.0.1:8765")); } } satisfies ProxyOptions]))
  },
  build: {
    outDir: "../src/schedule_management/web/static",
    emptyOutDir: true
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"]
  }
});
