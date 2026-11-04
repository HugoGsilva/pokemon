import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // O build do Rust trava arquivos em src-tauri/target (EBUSY) e derruba o watcher do Vite
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // Barra de controle (main), janela Ferramentas (tools) e página do servidor remoto (remote, ver server/)
    rollupOptions: {
      input: {
        main: "index.html",
        tools: "tools.html",
        remote: "remote.html",
      },
    },
    target: "esnext",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
