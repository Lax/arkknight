<script setup lang="ts">
import { ref } from 'vue'
import { NCard, NSpace, NTag, NText } from 'naive-ui'

// M1 调度策略经 arkknight.toml 管理（单一事实源，INV-4）；控制台 v1 只读展示，
// 策略编辑器属任务 8 尾批（连同 OpenAPI/统计页）
const items = [
  { key: '时间窗/优先级/时间片', value: 'accounts/<id>/account.toml（控制台-账号页可改启停/优先级）' },
  { key: '游戏日界', value: 'arkknight.toml scheduler.game_day_boundary（默认 04:00，官服 UTC-4）' },
  { key: '默认时间片', value: 'arkknight.toml scheduler.default_slice（默认 2h）' },
  { key: '会话硬上限', value: 'arkknight.toml scheduler.max_session_runtime（默认 6h）' },
  { key: '看门狗', value: 'watchdog_interval=30s，连续 3 次失败 → drain' },
  { key: '优雅停止', value: 'drain_grace=2m（mower POST /stop → 超时强杀）' },
  { key: '退避', value: 'initial=5m factor=2 max=60m（失败指数退避）' },
  { key: '暂停', value: 'Dashboard 或 arkknight schedule pause/resume（运行中会话不受影响）' },
]
</script>

<template>
  <n-card title="调度策略（只读视图）" size="small">
    <n-space vertical>
      <n-tag type="info">M1 策略经工作目录文件管理（INV-4 单一事实源）；表单化编辑属任务 8 尾批</n-tag>
      <div v-for="i in items" :key="i.key" style="display: flex; gap: 12px">
        <n-text strong style="min-width: 180px">{{ i.key }}</n-text>
        <span style="color: gray; font-family: monospace; font-size: 13px">{{ i.value }}</span>
      </div>
    </n-space>
  </n-card>
</template>
