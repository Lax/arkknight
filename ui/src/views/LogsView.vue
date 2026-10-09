<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { NButton, NCard, NInput, NInputNumber, NSpace, NSelect, NTag, useMessage } from 'naive-ui'
import { api, wsUrl } from '../api'

const message = useMessage()
const accounts = ref<{ label: string; value: string }[]>([])
const account = ref<string | null>(null)
const executor = ref('mower')
const sliceMin = ref<number | null>(null)
const task = ref('')
const logId = ref<number | null>(null)
const logLines = ref<string[]>([])
let ws: WebSocket | null = null

const executorOptions = [
  { label: 'mower（基建）', value: 'mower' },
  { label: 'maa（任务）', value: 'maa' },
]

async function refresh(): Promise<void> {
  try {
    const list = (await api.accounts()) ?? []
    accounts.value = list.map((a) => ({ label: `${a.key}（${a.enabled ? '启用' : '停用'}）`, value: a.key }))
  } catch (e) {
    message.error(`账号读取失败：${(e as Error).message}`)
  }
}

async function start(): Promise<void> {
  if (!account.value) {
    message.error('请选择账号')
    return
  }
  try {
    await api.startSession(
      account.value,
      executor.value,
      sliceMin.value ? `${sliceMin.value}m` : undefined,
      task.value || undefined,
    )
    message.success('手动会话已发起（202）')
  } catch (e) {
    message.error((e as Error).message)
  }
}

function follow(): void {
  ws?.close()
  if (!logId.value) {
    message.error('请输入会话 id')
    return
  }
  logLines.value = []
  ws = new WebSocket(wsUrl(`logs=${logId.value}`))
  ws.onmessage = (ev) => {
    logLines.value.push(String(ev.data))
    if (logLines.value.length > 3000) logLines.value.splice(0, 1000)
  }
  ws.onerror = () => message.error('日志连接失败')
}

onMounted(refresh)
onBeforeUnmount(() => ws?.close())
</script>

<template>
  <n-space vertical size="large">
    <n-card title="手动会话（插队，不抢占运行中会话）" size="small">
      <n-space align="center" wrap>
        <n-select
          v-model:value="account"
          :options="accounts"
          placeholder="选择账号"
          style="width: 220px"
          filterable
        />
        <n-select v-model:value="executor" :options="executorOptions" style="width: 150px" />
        <n-input-number v-model:value="sliceMin" :min="1" placeholder="时间片（分钟，可空）" style="width: 190px" />
        <n-input v-if="executor === 'maa'" v-model:value="task" placeholder="maa 任务名" style="width: 160px" />
        <n-button type="primary" @click="start">发起会话</n-button>
      </n-space>
    </n-card>

    <n-card title="日志实时查看（WS tail）" size="small">
      <n-space align="center">
        <n-input-number v-model:value="logId" :min="1" placeholder="会话 id" style="width: 140px" />
        <n-button @click="follow">连接</n-button>
        <n-tag type="info">daemon 经 WebSocket 推送 logs/sessions/&lt;id&gt;.log 增量</n-tag>
      </n-space>
      <pre
        style="margin-top: 12px; font-size: 12px; line-height: 1.5; white-space: pre-wrap; word-break: break-all; max-height: 480px; overflow: auto"
      >{{ logLines.join('\n') }}</pre>
    </n-card>
  </n-space>
</template>
