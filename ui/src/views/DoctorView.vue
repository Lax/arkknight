<script setup lang="ts">
import { ref } from 'vue'
import { NButton, NCard, NSpace, NTag, useMessage } from 'naive-ui'
import { api } from '../api'

const message = useMessage()
const checks = ref<
  { id: string; level: string; detail: string; hint: string | null }[]
>([])
const loading = ref(false)

async function run(): Promise<void> {
  loading.value = true
  try {
    const r = (await api.doctor()) as { checks: typeof checks.value }
    checks.value = r.checks ?? []
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    loading.value = false
  }
}

function levelType(level: string): 'success' | 'warning' | 'error' {
  return level === 'ok' ? 'success' : level === 'warn' ? 'warning' : 'error'
}
function levelMark(level: string): string {
  return level === 'ok' ? '✓' : level === 'warn' ? '!' : '✗'
}
</script>

<template>
  <n-card title="维护中心 · 环境体检（doctor）" size="small">
    <template #header-extra>
      <n-button size="small" :loading="loading" @click="run">运行体检</n-button>
    </template>
    <n-space vertical>
      <n-tag v-if="!checks.length" type="default">点击「运行体检」开始（adb/设备/maa/mower/docker/账号一致性/端口段）</n-tag>
      <div v-for="c in checks" :key="c.id + c.detail" style="display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap">
        <n-tag size="small" :type="levelType(c.level)">{{ levelMark(c.level) }} {{ c.id }}</n-tag>
        <span>{{ c.detail }}</span>
        <n-text v-if="c.hint" depth="3" style="font-size: 12px">↳ {{ c.hint }}</n-text>
      </div>
    </n-space>
  </n-card>
</template>
