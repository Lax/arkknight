<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import {
  NButton, NCard, NDrawer, NDrawerContent, NInput, NSpace, NTag, NText, useMessage,
} from 'naive-ui'
import { api, wsUrl, type SessionInfo, fmtMs } from '../api'

const message = useMessage()
const sessions = ref<SessionInfo[]>([])
const logOpen = ref(false)
const logSessionId = ref<number | null>(null)
const logLines = ref<string[]>([])
let ws: WebSocket | null = null
let timer: number | undefined

async function refresh(): Promise<void> {
  try {
    sessions.value = (await api.sessions(50)) ?? []
  } catch (e) {
    message.error(`刷新失败：${(e as Error).message}`)
  }
}

function openLogs(id: number): void {
  ws?.close()
  logSessionId.value = id
  logLines.value = []
  logOpen.value = true
  ws = new WebSocket(wsUrl(`logs=${id}`))
  ws.onmessage = (ev) => {
    logLines.value.push(String(ev.data))
    if (logLines.value.length > 2000) logLines.value.splice(0, 500)
  }
  ws.onerror = () => message.error('日志连接失败')
}

function closeLogs(): void {
  ws?.close()
  ws = null
}

async function drain(id: number): Promise<void> {
  try {
    await api.drain(id)
    message.success(`会话 #${id} 已请求停止`)
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

onMounted(() => {
  void refresh()
  timer = window.setInterval(refresh, 4000)
})
onBeforeUnmount(() => {
  window.clearInterval(timer)
  closeLogs()
})
</script>

<template>
  <n-space vertical size="large">
    <n-card title="会话列表" size="small">
      <n-space vertical>
        <n-tag v-if="!sessions.length" type="default">暂无会话（会话由调度器或手动发起）</n-tag>
        <div
          v-for="s in sessions"
          :key="s.id"
          style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap"
        >
          <n-tag
            size="small"
            :type="s.state === 'running' ? 'success' : s.state === 'failed' ? 'error' : 'default'"
          >
            #{{ s.id }} {{ s.state }}
          </n-tag>
          <n-text strong>{{ s.account_key }}</n-text>
          <span style="color: gray">{{ s.executor }} @ {{ s.device_name }}</span>
          <span v-if="s.mower_port" style="color: gray">:{{ s.mower_port }}</span>
          <span style="color: gray">{{ fmtMs(s.started_at_ms) }}</span>
          <n-tag v-if="s.outcome" size="small" type="info">{{ s.outcome }}</n-tag>
          <n-text v-if="s.error" type="error" depth="3" style="font-size: 12px">{{ s.error }}</n-text>
          <n-button v-if="['running', 'switching', 'draining', 'queued'].includes(s.state)" size="tiny" @click="drain(s.id)">
            停止
          </n-button>
          <n-button size="tiny" @click="openLogs(s.id)">日志</n-button>
        </div>
      </n-space>
    </n-card>

    <n-drawer v-model:show="logOpen" :width="720" placement="right" @after-leave="closeLogs">
      <n-drawer-content :title="`会话 #${logSessionId ?? ''} 日志（实时）`" closable>
        <pre style="font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-all">{{
          logLines.join('\n')
        }}</pre>
      </n-drawer-content>
    </n-drawer>
  </n-space>
</template>
