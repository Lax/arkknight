# ADR-0005：Web 控制台鉴权只作用于 `/api/*`

- **状态**：已接受
- **日期**：2026-10-09
- **关联**：`../arkreunion-design.md` §17（安全考量）

## 背景

`arkreunion server` 的鉴权中间件 `auth_mw` 原先挂在整个 Router 上
（`.layer(middleware::from_fn_with_state(state, auth_mw))`），无差别拦截所有路由 ——
包括 `/assets/*` 静态资源。

当 `server.token` 非空时，这导致控制台**永久白屏**：

1. 用户访问 <http://127.0.0.1:7100/?token=xxx> → 首页 200（token 在查询串里）
2. 浏览器解析 HTML，加载 `<script src="/assets/index-*.js">` → **该请求不带
   `?token=`**（token 只存在于地址栏与 localStorage）
3. `auth_mw` 拦下 → 401 `{"error":"token 无效或缺失"}`
4. JS 未加载 → 页面空白；用户看到的是 `{"error":...}` 纯文本或白屏，
   完全无法判断「是 token 错了还是别的」

附带第二个问题：前端 `getToken()` 只读 localStorage，不读 URL 参数。所以即便用户
正确地在地址栏带了 `?token=`，首页加载后所有 API 调用仍会 401。

## 决策

1. **鉴权范围收敛到 `/api/*`**：静态资源不经鉴权。
2. **前端 `getToken()` 兜底读 URL 参数**并落入 localStorage。
3. 401 与其他错误一律带 `hint`（见 ADR-0006 的错误规范）。

## 备选与否决理由

- **给静态资源也豁免但要求 `<script>` 带 token**（如 meta 标签注入）：
  需要改构建产物（Vite 配置 + index.html 模板），且 CDN/浏览器缓存会让
  token 固化在产物里，token 轮换即失效。
- **用 Cookie 承载 token**：控制台与 API 同源，Cookie 可自动携带，
  实现上更「标准」。但需处理 HttpOnly/CSRF/过期等一堆问题，而本项目控制台
  仅在 loopback 或受控网络暴露（设计 §17），用查询串 + localStorage 已足够。
- **把静态资源移到独立端口/路径前缀**：架构改动过大，为一个鉴权范围问题不值得。
- **不鉴权静态资源但也不改前端**：只解决白屏，URL 带 token 首访仍失败。两条一起改才闭环。

## 后果

✅ 控制台可用：首页与 `/assets/*` 均 200，`/api/*` 仍严格校验
（无 token → 401，带对 token → 200）。

⚠️ 静态资源不再受鉴权保护。评估后认为可接受：

- 静态资源是构建期固定的公开 JS/CSS，不含任何敏感数据
- 账号凭据、UID、日志等业务数据一律走 `/api/*`，仍在鉴权后
- 控制台默认绑 `127.0.0.1`；暴露局域网时设计 §17 已强制要求配 token，
  而 token 保护的是 API 读写能力，不影响 UI 外壳的可见性

⚠️ 新增了回归测试 `静态资源不经鉴权_控制台可加载` 守着这条边界 —— 将来若有人
把 `auth_mw` 的范围改回全站，测试会失败。