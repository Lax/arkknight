import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// 开发期代理到 arkreunion server（M1 任务 8 起可用）；构建产物嵌入二进制
export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:7100',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
