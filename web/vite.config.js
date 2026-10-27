import path from 'path'
import { fileURLToPath } from 'url'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

/**
 * 给 favicon / touch icon 追加构建版本号（`?v=<build>`），
 * 规避浏览器与 CDN 对图标的长缓存导致换图标后不生效。
 */
function iconVersionPlugin() {
  const version = Date.now().toString(36)
  const stamp = (html, href) => {
    const escaped = href.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    return html.replace(new RegExp(`${escaped}(\\?v=[^"']*)?`, 'g'), `${href}?v=${version}`)
  }
  return {
    name: 'solidway-icon-version',
    transformIndexHtml(html) {
      return stamp(stamp(html, '/favicon.svg'), '/apple-touch-icon.png')
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss(), iconVersionPlugin()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 6573,
    proxy: {
      // 开发期把 /api 与 /v1 代理到 solidway 后端，接入信息里的 Base URL 可直接使用。
      '/api': 'http://127.0.0.1:6565',
      '/v1': 'http://127.0.0.1:6565',
    },
  },
  preview: {
    port: 6573,
  },
})
