// arkknight API 客户端：token 存 localStorage（控制台设置区可改），经 Bearer 头发送
export function getToken(): string {
  const stored = localStorage.getItem('arkknight-token') ?? ''
  if (stored) return stored
  // 首次用 http://host:port/?token=xxx 打开时，URL 参数落到 localStorage 供后续请求用
  const fromUrl = new URLSearchParams(location.search).get('token') ?? ''
  if (fromUrl) {
    localStorage.setItem('arkknight-token', fromUrl)
    return fromUrl
  }
  return ''
}

export function setToken(t: string): void {
  localStorage.setItem('arkknight-token', t)
}

async function req(method: string, path: string, body?: unknown): Promise<unknown> {
  const headers: Record<string, string> = {}
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  const t = getToken()
  if (t) headers['Authorization'] = `Bearer ${t}`
  const res = await fetch(path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) {
    const data = (await res.json().catch(() => ({}))) as {
      error?: string
      hint?: string
      config_path?: string
    }
    // 401 单独标记：控制台据此在侧栏提示「token 不对」，而非每个页面各弹一次
    if (res.status === 401) {
      localStorage.setItem('arkknight-auth-failed', '1')
    }
    // 服务端错误带 hint 时一并抛出：hint 说明「去哪里改」，比裸 error 更可操作
    const parts = [data.error ?? `HTTP ${res.status}`]
    if (data.hint) parts.push(data.hint)
    throw new Error(parts.join('\n'))
  }
  const ct = res.headers.get('content-type') ?? ''
  return ct.includes('json') ? res.json() : res.text()
}

export interface AccountInfo {
  /** 本地定位键（= 目录名 accounts/<key>/），非游戏身份 */
  key: string
  display_name: string
  server: string
  /** MAA 切号匹配串：官服=打码手机号片段，B服=昵称 */
  account_name: string
  /** 游戏 UID（纯数字）：切号后 OCR 核验防串数据；null = 未配置，跳过核验 */
  uid: string | null
  enabled: boolean
  priority: number
  slice: string | null
  /** mower 会话运行形态：process（默认）| docker */
  runner: string
  windows: { start: string; end: string; executor: string; task: string | null }[]
  provisioned_on: string[]
}

export interface SessionInfo {
  id: number
  account_key: string
  device_name: string
  executor: string
  state: string
  mower_port: number | null
  started_at_ms: number | null
  ended_at_ms: number | null
  outcome: string | null
  error: string | null
}

export interface DeviceInfo {
  name: string
  backend: string
  host_adb: string
  docker_adb: string | null
  docker_network: string | null
  docker_compatible: boolean
  notes: string
}

export interface StatusInfo {
  paused: boolean
  active_session: number | null
  accounts: AccountInfo[]
  devices: { name: string; host_adb: string }[]
}

export interface ProcessInfo {
  source: "managed" | "external"
  kind: string
  session_id?: number
  account?: string
  device?: string
  state?: string
  pid?: number | null
  mower_port?: number | null
  alive?: boolean
  started_at_ms?: number | null
  deep_link?: string | null
  cmdline?: string
}

export const api = {
  processes: (external = true) =>
    req('GET', `/api/processes?external=${external ? 1 : 0}`) as Promise<{
      managed: ProcessInfo[]
      external: ProcessInfo[]
    }>,
  status: () => req('GET', '/api/status') as Promise<StatusInfo>,
  sessions: (limit = 30) => req('GET', `/api/sessions?limit=${limit}`) as Promise<SessionInfo[]>,
  sessionLogs: (id: number, tail = 300) =>
    req('GET', `/api/sessions/${id}/logs?tail=${tail}`) as Promise<string>,
  startSession: (account: string, executor = 'mower', slice?: string, task?: string) =>
    req('POST', '/api/sessions', { account, executor, slice, task }),
  drain: (id: number) => req('POST', `/api/sessions/${id}/drain`),
  accounts: () => req('GET', '/api/accounts') as Promise<AccountInfo[]>,
  createAccount: (body: {
    key: string
    display_name?: string | null
    server: string
    account_name: string
    uid?: string | null
    priority?: number
    windows?: unknown[]
  }) => req('POST', '/api/accounts', body),
  account: (key: string) => req('GET', `/api/accounts/${key}`) as Promise<AccountInfo>,
  patchAccount: (
    key: string,
    body: {
      enabled?: boolean
      priority?: number
      account_name?: string
      display_name?: string
      uid?: string
      server?: string
      /** 整体替换；[] = 清空（退出自动调度） */
      windows?: { start: string; end: string; executor: string; task?: string | null }[]
      /** undefined=不变；null/''=回全局默认；'90m' 等=覆盖 */
      slice?: string | null
      runner?: string
    },
  ) => req('PATCH', `/api/accounts/${key}`, body),
  getPlan: (key: string) =>
    req('GET', `/api/accounts/${key}/plan`) as Promise<{
      exists: boolean
      content: string | null
      path: string
    }>,
  putPlan: (key: string, content: string) =>
    req('PUT', `/api/accounts/${key}/plan`, { content }) as Promise<{ ok: boolean; path: string }>,
  maaTasks: (key: string) =>
    req('GET', `/api/accounts/${key}/maa-tasks`) as Promise<{
      tasks: { name: string; size: number }[]
      dir: string
    }>,
  maaTask: (key: string, name: string) =>
    req('GET', `/api/accounts/${key}/maa-tasks/${encodeURIComponent(name)}`) as Promise<{
      name: string
      content: string
      path: string
    }>,
  putMaaTask: (key: string, name: string, content: string) =>
    req('PUT', `/api/accounts/${key}/maa-tasks/${encodeURIComponent(name)}`, {
      content,
    }) as Promise<{ ok: boolean; path: string }>,
  deleteMaaTask: (key: string, name: string) =>
    req(
      'DELETE',
      `/api/accounts/${key}/maa-tasks/${encodeURIComponent(name)}?confirm=1`,
    ) as Promise<{ ok: boolean }>,
  deleteAccount: (key: string) => req('DELETE', `/api/accounts/${key}?confirm=1`),
  devices: () => req('GET', '/api/devices') as Promise<DeviceInfo[]>,
  testDevice: (name: string) => req('POST', `/api/devices/${name}/test`),
  /** 设备截图 URL：作为 <img src> 消费，故不经 fetch（带 token 的查询串）。
      cacheBuster 用于手动刷新，绕开 no-store 之外的浏览器缓存。 */
  screenshotUrl: (name: string, nonce = 0) =>
    `/api/devices/${encodeURIComponent(name)}/screenshot`
      + `?token=${encodeURIComponent(getToken())}`
      + (nonce ? `&t=${nonce}` : ''),
  doctor: () => req('GET', '/api/doctor'),
  /** 调度展示参数：游戏日界 / 时区 / 默认时间片 / 本地定时事项 */
  schedulerInfo: () =>
    req('GET', '/api/local-events') as Promise<{
      game_day_boundary: string
      timezone: string
      default_slice: string
      local_events: { name: string; start: string; end: string }[]
    }>,
  /** 整体替换本地定时事项（时间轴标注用） */
  putLocalEvents: (events: { name: string; start: string; end: string }[]) =>
    req('PUT', '/api/local-events', events) as Promise<{ ok: boolean }>,
  pause: () => req('POST', '/api/schedule/pause'),
  resume: () => req('POST', '/api/schedule/resume'),
}

export function wsUrl(query: string): string {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const t = getToken()
  return `${proto}://${location.host}/api/ws?${query}${t ? `&token=${encodeURIComponent(t)}` : ''}`
}

export function fmtMs(ms: number | null): string {
  if (!ms) return '—'
  return new Date(ms).toLocaleString()
}
