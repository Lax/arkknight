// akops API 客户端：token 存 localStorage（控制台设置区可改），经 Bearer 头发送
export function getToken(): string {
  return localStorage.getItem('akops-token') ?? ''
}

export function setToken(t: string): void {
  localStorage.setItem('akops-token', t)
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
    const data = (await res.json().catch(() => ({}))) as { error?: string }
    throw new Error(data.error ?? `HTTP ${res.status}`)
  }
  const ct = res.headers.get('content-type') ?? ''
  return ct.includes('json') ? res.json() : res.text()
}

export interface AccountInfo {
  id: string
  display_name: string
  server: string
  account_name: string
  enabled: boolean
  priority: number
  slice: string | null
  windows: { start: string; end: string; executor: string; task: string | null }[]
  provisioned_on: string[]
}

export interface SessionInfo {
  id: number
  account_id: string
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

export const api = {
  status: () => req('GET', '/api/status') as Promise<StatusInfo>,
  sessions: (limit = 30) => req('GET', `/api/sessions?limit=${limit}`) as Promise<SessionInfo[]>,
  sessionLogs: (id: number, tail = 300) =>
    req('GET', `/api/sessions/${id}/logs?tail=${tail}`) as Promise<string>,
  startSession: (account: string, executor = 'mower', slice?: string, task?: string) =>
    req('POST', '/api/sessions', { account, executor, slice, task }),
  drain: (id: number) => req('POST', `/api/sessions/${id}/drain`),
  accounts: () => req('GET', '/api/accounts') as Promise<AccountInfo[]>,
  createAccount: (body: unknown) => req('POST', '/api/accounts', body),
  patchAccount: (id: string, body: unknown) => req('PATCH', `/api/accounts/${id}`, body),
  deleteAccount: (id: string) => req('DELETE', `/api/accounts/${id}?confirm=1`),
  devices: () => req('GET', '/api/devices') as Promise<DeviceInfo[]>,
  testDevice: (name: string) => req('POST', `/api/devices/${name}/test`),
  doctor: () => req('GET', '/api/doctor'),
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
