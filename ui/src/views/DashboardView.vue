<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { NButton, NCard, NInput, NSpace, NTag, useMessage } from 'naive-ui'
import { getToken, setToken, api, type StatusInfo, type SessionInfo, fmtMs } from '../api'

const message = useMessage()
const tokenInput = ref(getToken())
const status = ref<StatusInfo | null>(null)
const sessions = ref<SessionInfo[]>([])
let timer: number | undefined

async function refresh(): Promise<void> {
  try {
    status.value = await api.status()
    sessions.value = (await api.sessions(8)) ?? []
  } catch (e) {
    message.error(`刷新失败：${(e as Error).message}`)
  }
}

function saveToken(): void {
  setToken(tokenInput.value.trim())
  message.success('token 已保存')
  void refresh()
}

async function togglePause(): Promise<void> {
  try {
    if (status.value?.paused) {
      await api.resume()
    } else {
      await api.pause()
    }
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

onMounted(() => {
  void refresh()
  timer = window.setInterval(refresh, 4000)
})
onBeforeUnmount(() => window.clearInterval(timer))
</script>

<template>
  <n-space vertical size="large">
    <n-card title="运行状态" size="small">
      <n-space vertical>
        <n-space align="center">
          <n-tag :type="status ? 'success' : 'error'">daemon</n-tag>
          <n-tag :type="status?.paused ? 'warning' : 'info'">
            调度器{{ status?.paused ? '已暂停' : '运行中' }}
          </n-tag>
          <n-tag type="default">活跃会话 #{{ status?.active_session ?? '—' }}</n-tag>
          <n-button size="small" @click="togglePause">
            {{ status?.paused ? '恢复调度' : '暂停调度' }}
          </n-button>
        </n-space>
        <n-space align="center">
          <span>API token：</span>
          <n-input
            v-model:value="tokenInput"
            size="small"
            type="password"
            show-password-on="click"
            placeholder="server.token（未设可留空）"
            style="width: 260px"
          />
          <n-button size="small" @click="saveToken">保存</n-button>
        </n-space>
      </n-space>
    </n-card>

    <n-card title="最近会话" size="small">
      <n-space vertical>
        <n-tag v-if="!sessions.length" type="default">暂无会话记录</n-tag>
        <div v-for="s in sessions" :key="s.id" style="display: flex; gap: 12px; align-items: center">
          <n-tag
            size="small"
            :type="s.state === 'running' ? 'success' : s.state === 'failed' ? 'error' : 'default'"
          >
            #{{ s.id }} {{ s.state }}
          </n-tag>
          <span>{{ s.account_id }}</span>
          <span style="color: gray">{{ s.executor }} @ {{ s.device_name }}</span>
          <span style="color: gray">{{ fmtMs(s.started_at_ms) }}</span>
          <n-tag v-if="s.outcome" size="small" type="info">{{ s.outcome }}</n-tag>
        </div>
      </n-space>
    </n-card>
  </n-space>
</template>
