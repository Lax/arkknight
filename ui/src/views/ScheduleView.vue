<script setup lang="ts">
import { onMounted, ref } from 'vue'
import {
  NAlert, NButton, NCard, NInput, NSpace, NTag, NText, useMessage,
} from 'naive-ui'
import { api } from '../api'
import TimeLine24 from '../components/TimeLine24.vue'

const message = useMessage()

const boundary = ref('04:00')
const timezone = ref('')
const defaultSlice = ref('')
const events = ref<{ name: string; start: string; end: string }[]>([])
const saving = ref(false)

// 只读示例窗：展示事项在时间轴上的观感（不落盘）
const previewWindows = ref<{ start: string; end: string; executor: string; task?: string | null }[]>([])

const items = [
  { key: '游戏日界', value: 'arkknight.toml scheduler.game_day_boundary（默认 04:00，官服 UTC-4）' },
  { key: '默认时间片', value: 'arkknight.toml scheduler.default_slice（默认 2h）' },
  { key: '会话硬上限', value: 'arkknight.toml scheduler.max_session_runtime（默认 6h）' },
  { key: '看门狗', value: 'watchdog_interval=30s，连续 3 次失败 → drain' },
  { key: '优雅停止', value: 'drain_grace=2m（mower POST /stop → 超时强杀）' },
  { key: '退避', value: 'initial=5m factor=2 max=60m（失败指数退避）' },
  { key: '暂停', value: 'Dashboard 或 arkknight schedule pause/resume（运行中会话不受影响）' },
]

async function load(): Promise<void> {
  try {
    const info = await api.schedulerInfo()
    boundary.value = info.game_day_boundary
    timezone.value = info.timezone
    defaultSlice.value = info.default_slice
    events.value = info.local_events.map((e) => ({ ...e }))
  } catch (e) {
    message.error(`加载失败：${(e as Error).message}`)
  }
}

function addEvent(): void {
  // 多段事项：延续上一条名称（同一事项的多个时段录多条）
  const last = events.value[events.value.length - 1]
  events.value.push({ name: last?.name ?? '', start: '', end: '' })
}

async function save(): Promise<void> {
  const clean = events.value
    .filter((e) => e.name.trim() || e.start || e.end)
    .map((e) => ({ name: e.name.trim(), start: e.start.trim(), end: e.end.trim() }))
  saving.value = true
  try {
    await api.putLocalEvents(clean)
    message.success('本地定时事项已保存（各账号时间轴即时可见）')
    events.value = clean
    await load()
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    saving.value = false
  }
}

onMounted(load)
</script>

<template>
  <n-space vertical size="large">
    <n-card title="本地定时事项" size="small">
      <template #header-extra>
        <n-space align="center" size="small">
          <n-tag size="small" type="info">游戏日界 {{ boundary }}（{{ timezone }}）</n-tag>
          <n-button size="small" @click="addEvent">+ 加一项</n-button>
          <n-button type="primary" size="small" :loading="saving" @click="save">保存</n-button>
        </n-space>
      </template>
      <n-space vertical size="small">
        <n-text depth="3" style="font-size: 12px">
          每日本地定时事项（如「每晚 18:00 网络闪断五分钟」「路由器 03:30 定时重启」）：
          会标注在各账号时间轴上供选窗避让。**同一事项可有多个时段**——加一项会自动
          带上上一条的名称，逐段填写即可；end 小于 start 即跨过自然午夜（如
          23:50-00:10）。当前版本仅作标注提示，调度器不强制避让——短时闪断内
          mower 的 adb 会自动重连，通常无需处理。
        </n-text>
        <div v-for="(e, i) in events" :key="i" class="event-row">
          <n-input v-model:value="e.name" placeholder="名称，如 网络闪断" style="flex: 1; min-width: 140px" />
          <n-input v-model:value="e.start" placeholder="18:00" style="width: 90px" />
          <span style="color: gray">–</span>
          <n-input v-model:value="e.end" placeholder="00:10（&lt;start=跨天）" style="width: 120px" />
          <n-button size="tiny" type="error" @click="events.splice(i, 1)">删</n-button>
        </div>
        <n-empty v-if="!events.length" description="暂无本地事项" size="small" />
      </n-space>
    </n-card>

    <n-card title="时间轴预览" size="small">
      <TimeLine24
        :windows="previewWindows"
        :boundary="boundary"
        :events="events"
        readonly
      />
    </n-card>

    <n-card title="调度策略（只读视图）" size="small">
      <n-space vertical>
        <n-tag type="info">M1 策略经工作目录文件管理（INV-4 单一事实源）；表单化编辑属任务 8 尾批</n-tag>
        <div v-for="i in items" :key="i.key" style="display: flex; gap: 12px">
          <n-text strong style="min-width: 180px">{{ i.key }}</n-text>
          <span style="color: gray; font-family: monospace; font-size: 13px">{{ i.value }}</span>
        </div>
      </n-space>
    </n-card>
  </n-space>
</template>

<style scoped>
.event-row {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
</style>
