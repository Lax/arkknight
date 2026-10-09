<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { NButton, NCard, NSpace, NTag, NText, useMessage } from 'naive-ui'
import { getToken, api, type StatusInfo, type SessionInfo, type ProcessInfo, fmtMs } from '../api'

const message = useMessage()
const hasToken = ref(getToken().length > 0)
const status = ref<StatusInfo | null>(null)
const sessions = ref<SessionInfo[]>([])
const procs = ref<{ managed: ProcessInfo[]; external: ProcessInfo[] } | null>(null)
let timer: number | undefined

async function refresh(): Promise<void> {
  try {
    status.value = await api.status()
    sessions.value = (await api.sessions(8)) ?? []
    procs.value = await api.processes(true)
  } catch (e) {
    message.error(`刷新失败：${(e as Error).message}`)
  }
  // token 可能在左栏改过，每次刷新同步一次
  hasToken.value = getToken().length > 0
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
        <!-- token 设置已移至左侧栏（窄屏不挤压主区），此处仅作状态提示 -->
        <n-space align="center" size="small">
          <n-tag :type="hasToken ? 'success' : 'default'" size="small">
            {{ hasToken ? 'API token 已设置' : '未设 token' }}
          </n-tag>
          <n-text depth="3" style="font-size: 12px">
            在左侧栏「API token」处配置
          </n-text>
        </n-space>
      </n-space>
    </n-card>

    <n-card title="运行中的进程" size="small">
      <n-space vertical>
        <n-tag v-if="!procs || (!procs.managed.length && !procs.external.length)" type="default">
          当前没有运行中的 mower / maa 进程
        </n-tag>
        <div v-for="(p, i) in procs?.managed ?? []" :key="'m' + i" style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap">
          <n-tag size="small" :type="p.alive ? 'success' : 'error'">{{ p.alive ? '运行中' : '已退出' }}</n-tag>
          <n-tag size="small" type="info">{{ p.kind }}</n-tag>
          <n-text strong>{{ p.account }}</n-text>
          <span style="color: gray">会话 #{{ p.session_id }} @ {{ p.device }}</span>
          <span style="color: gray">pid={{ p.pid ?? '—' }}</span>
          <a
            v-if="p.alive && p.deep_link"
            :href="p.deep_link"
            target="_blank"
            style="color: #4098fc"
          >mower UI :{{ p.mower_port }} ↗</a>
        </div>
        <n-text v-if="procs?.external?.length" depth="3" style="font-size: 12px">
          未托管进程（非 arkknight 启动，仅供参考）：
        </n-text>
        <div v-for="(p, i) in procs?.external ?? []" :key="'e' + i" style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap">
          <n-tag size="small" type="warning">未托管</n-tag>
          <n-tag size="small" type="default">{{ p.kind }}</n-tag>
          <span style="color: gray">pid={{ p.pid }}</span>
          <n-text depth="3" style="font-size: 12px">{{ p.cmdline }}</n-text>
        </div>
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
          <span>{{ s.account_key }}</span>
          <span style="color: gray">{{ s.executor }} @ {{ s.device_name }}</span>
          <span style="color: gray">{{ fmtMs(s.started_at_ms) }}</span>
          <n-tag v-if="s.outcome" size="small" type="info">{{ s.outcome }}</n-tag>
        </div>
      </n-space>
    </n-card>
  </n-space>
</template>
