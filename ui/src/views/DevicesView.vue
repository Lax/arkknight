<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NButton, NCard, NSpace, NTag, NText, useMessage } from 'naive-ui'
import { api, type DeviceInfo } from '../api'

const message = useMessage()
const devices = ref<DeviceInfo[]>([])
const testing = ref<string | null>(null)
const results = ref<Record<string, string>>({})

async function refresh(): Promise<void> {
  try {
    devices.value = (await api.devices()) ?? []
  } catch (e) {
    message.error(`刷新失败：${(e as Error).message}`)
  }
}

async function test(d: DeviceInfo): Promise<void> {
  testing.value = d.name
  try {
    const r = (await api.testDevice(d.name)) as {
      reachable: boolean
      detail: string
      game_packages: string[]
    }
    results.value[d.name] = r.reachable
      ? `在线（${r.detail}）；游戏包：${r.game_packages.join(', ') || '未检出'}`
      : `不在线：${r.detail}`
  } catch (e) {
    results.value[d.name] = `测试失败：${(e as Error).message}`
  } finally {
    testing.value = null
  }
}

onMounted(refresh)
</script>

<template>
  <n-card title="设备管理" size="small">
    <n-space vertical>
      <n-tag v-if="!devices.length" type="default">未注册设备（CLI：akops device add）</n-tag>
      <div v-for="d in devices" :key="d.name" style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap">
        <n-tag size="small" :type="d.backend === 'external' ? 'info' : 'warning'">{{ d.backend }}</n-tag>
        <n-text strong>{{ d.name }}</n-text>
        <span style="color: gray">host={{ d.host_adb }}</span>
        <span v-if="d.docker_adb" style="color: gray">docker={{ d.docker_adb }}</span>
        <n-tag v-if="d.docker_compatible" size="small" type="success">docker✓</n-tag>
        <n-button size="tiny" :loading="testing === d.name" @click="test(d)">测试</n-button>
        <n-text v-if="results[d.name]" depth="2" style="font-size: 12px">{{ results[d.name] }}</n-text>
      </div>
    </n-space>
  </n-card>
</template>
