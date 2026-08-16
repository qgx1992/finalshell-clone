import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  // Tauri 期望固定端口,若被占用自动换端口会失败
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // codemirror chunk 1.7MB 是懒加载的(仅编辑器打开时加载),属预期
    chunkSizeWarningLimit: 1700,
    rollupOptions: {
      output: {
        // 按大依赖分包:EditorModal 懒加载后 CodeMirror 只在需要时加载
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (id.includes("@xterm")) return "xterm";
          if (
            id.includes("codemirror") ||
            id.includes("@codemirror") ||
            id.includes("@lezer") ||
            id.includes("crelt") ||
            id.includes("style-mod") ||
            id.includes("w3c-keyname")
          )
            return "codemirror";
          // react/react-dom/scheduler/jsx-runtime 一体化:react-dom 的 CJS 转换
          // 依赖 scheduler 同 chunk,拆分后压缩变量名冲突导致 React 初始化崩溃
          if (id.includes("react") || id.includes("scheduler")) return "react";
          if (id.includes("@tauri-apps")) return "tauri";
          return "vendor";
        },
      },
    },
  },
});
