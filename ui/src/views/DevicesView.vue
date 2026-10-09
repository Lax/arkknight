<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import {
  NButton, NCard, NSpace, NTag, NText, NSkeleton, NEmpty, useMessage,
} from 'naive-ui'
import { api, type DeviceInfo } from '../api'

const message = useMessage()
const devices = ref<DeviceInfo[]>([])
const testing = ref<string | null>(null)
const results = ref<Record<string, string>>({})

// 截图：按需拉取（不轮询，避免 adb 频繁截屏拖慢设备）
const shots = ref<Record<string, string>>({})
const shotErr = ref<Record<string, string>>({})
const shotLoading = ref<Record<string, boolean>>({})
let nonce = 0

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

function capture(d: DeviceInfo): void {
  shotLoading.value[d.name] = true
  const url = api.screenshotUrl(d.name, ++nonce)
  // 先预载再落 URL：加载失败时不显示破图，改为错误提示
  const img = new Image()
  img.onload = () => {
    shots.value[d.name] = url
    delete shotErr.value[d.name]
    shotLoading.value[d.name] = false
  }
  img.onerror = () => {
    delete shots.value[d.name]
    shotErr.value[d.name] = '截屏失败：设备离线或屏幕未点亮（先做「测试」）'
    shotLoading.value[d.name] = false
  }
  img.src = url
}

function refreshAllShots(): void {
  devices.value.forEach(capture)
}

onMounted(refresh)
onBeforeUnmount(() => {
  // 释放 object URL 之外的资源无需求；nonce 由组件生命周期自然结束
})
</script>

<template>
  <n-card title="设备管理" size="small">
    <template #header-extra>
      <n-space size="small">
        <n-button size="small" @click="refresh">刷新列表</n-button>
        <n-button size="small" :disabled="!devices.length" @click="refreshAllShots">
          全部截屏
        </n-button>
      </n-space>
    </template>

    <n-space vertical size="large">
      <n-empty v-if="!devices.length" description="未注册设备">
        <template #extra>
          <n-text depth="3" style="font-size: 12px">
            CLI：arkreunion device add &lt;name&gt; --host-adb &lt;宿主adb地址&gt;
          </n-text>
        </template>
      </n-empty>

      <n-card
        v-for="d in devices"
        :key="d.name"
        size="small"
        :bordered="true"
        style="min-width: 0"
      >
        <template #header>
          <n-space size="small" align="center" wrap>
            <n-tag size="small" :type="d.backend === 'external' ? 'info' : 'warning'">
              {{ d.backend }}
            </n-tag>
            <n-text strong>{{ d.name }}</n-text>
            <n-tag v-if="d.docker_compatible" size="small" type="success">docker✓</n-tag>
          </n-space>
        </template>
        <template #header-extra>
          <n-space size="small">
            <n-button size="tiny" :loading="testing === d.name" @click="test(d)">测试</n-button>
            <n-button
              size="tiny"
              :loading="shotLoading[d.name]"
              @click="capture(d)"
            >
              {{ shots[d.name] ? '刷新截图' : '截图' }}
            </n-button>
          </n-space>
        </template>

        <!-- 地址行：窄屏换行，避免横向溢出 -->
        <div
          style="
            display: flex; gap: 6px 14px; flex-wrap: wrap;
            font-size: 12px; color: gray; margin-bottom: 8px;
          "
        >
          <span>host={{ d.host_adb }}</span>
          <span v-if="d.docker_adb">docker={{ d.docker_adb }}</span>
          <span v-if="d.docker_network">network={{ d.docker_network }}</span>
          <span v-if="d.notes">{{ d.notes }}</span>
        </div>

        <n-text
          v-if="results[d.name]"
          depth="2"
          style="display: block; font-size: 12px; margin-bottom: 8px; word-break: break-all"
        >
          {{ results[d.name] }}
        </n-text>

        <div style="display: flex; gap: 12px; align-items: flex-start; flex-wrap: wrap">
          <div style="flex: 0 1 320px; min-width: 0; max-width: 100%">
            <n-skeleton v-if="shotLoading[d.name] && !shots[d.name]" height="180px" />
            <img
              v-else-if="shots[d.name]"
              :src="shots[d.name]"
              :alt="`${d.name} 设备画面`"
              style="
                width: 100%; height: auto; display: block;
                border: 1px solid var(--n-border-color, #eee);
                border-radius: 4px; background: #000;
              "
            />
            <n-text v-else-if="shotErr[d.name]" depth="3" style="font-size: 12px; color: #d03050">
              {{ shotErr[d.name] }}
            </n-text>
            <n-text v-else depth="3" style="font-size: 12px">
              未截屏 —— 点右上「截图」抓取当前设备画面
            </n-text>
          </div>
        </div>
      </n-card>
    </n-space>
  </n-card>
</template>