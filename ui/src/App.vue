<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import {
  NConfigProvider, NDateLocale, NLayout, NLayoutContent, NLayoutSider, NMenu, NMessageProvider,
  NInput, NSpace, NButton, zhCN, dateZhCN,
} from 'naive-ui'
import type { MenuOption } from 'naive-ui'
import { getToken, setToken } from './api'

const route = useRoute()
const tokenInput = ref(getToken())
// 401 提示：只认「鉴权失败」，其它业务错误不归咎于 token
const authFailed = ref(
  (localStorage.getItem('arkknight-auth-failed') ?? '0') === '1',
)

// 窄屏（<900px）自动收起侧栏，避免菜单挤占内容区导致横向滚动
const NARROW = 900
const collapsed = ref(false)
const viewportWidth = ref(typeof window === 'undefined' ? 1280 : window.innerWidth)
let mql: MediaQueryList | null = null

function applyViewport(): void {
  collapsed.value = viewportWidth.value < NARROW
}
function onResize(): void {
  viewportWidth.value = window.innerWidth
  // 用户手动展开后，窗口仍窄则保持展开（不反复抢状态）
  if (viewportWidth.value >= NARROW) collapsed.value = false
}

onMounted(() => {
  mql = window.matchMedia(`(max-width: ${NARROW - 1}px)`)
  applyViewport()
  mql.addEventListener('change', onResize)
  window.addEventListener('resize', onResize)
})
onBeforeUnmount(() => {
  mql?.removeEventListener('change', onResize)
  window.removeEventListener('resize', onResize)
})

// 内容区内边距随宽度收敛；并允许内部 flex 子项收缩（min-width:0）
const contentStyle = computed(() => ({
  padding: viewportWidth.value < 700 ? '10px' : '20px',
  maxWidth: '100%',
  overflowX: 'auto' as const,
}))

const menuOptions: MenuOption[] = [
  { label: () => h(RouterLink, { to: '/' }, { default: () => '总览' }), key: 'dashboard' },
  { label: () => h(RouterLink, { to: '/sessions' }, { default: () => '会话' }), key: 'sessions' },
  { label: () => h(RouterLink, { to: '/logs' }, { default: () => '日志' }), key: 'logs' },
  { label: () => h(RouterLink, { to: '/accounts' }, { default: () => '账号' }), key: 'accounts' },
  { label: () => h(RouterLink, { to: '/devices' }, { default: () => '设备' }), key: 'devices' },
  { label: () => h(RouterLink, { to: '/schedule' }, { default: () => '调度' }), key: 'schedule' },
  { label: () => h(RouterLink, { to: '/doctor' }, { default: () => '维护' }), key: 'doctor' },
]

function saveToken(): void {
  setToken(tokenInput.value.trim())
  localStorage.removeItem('arkknight-auth-failed')
  location.reload()
}
</script>

<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-layout has-sider style="height: 100vh; min-height: 0">
        <n-layout-sider
          bordered
          content-style="padding: 12px"
          :width="210"
          :collapsed-width="52"
          :collapsed="collapsed"
          :native-scrollbar="false"
          show-trigger
          @collapse="collapsed = true"
          @expand="collapsed = false"
        >
          <h2 v-if="!collapsed" style="margin: 4px 8px 14px; font-size: 15px; white-space: nowrap">
            arkknight 控制台
          </h2>
          <h2 v-else style="margin: 4px 0 14px; text-align: center; font-size: 15px">AR</h2>
          <n-menu :options="menuOptions" :value="String(route.name)" :collapsed="collapsed" />
          <n-space
            v-if="!collapsed"
            vertical
            style="margin-top: 18px; padding: 0 8px"
            size="small"
          >
            <span style="font-size: 12px; color: gray">API token</span>
            <n-input
              v-model:value="tokenInput"
              size="tiny"
              type="password"
              show-password-on="click"
              placeholder="未设可留空"
              @keyup.enter="saveToken"
            />
            <span style="font-size: 11px; color: gray; line-height: 1.5">
              值须与工作目录 arkknight.toml 的
              <code>[server].token</code> 一致，改配置后需重启 daemon
            </span>
            <n-button size="tiny" secondary :disabled="!tokenInput" @click="saveToken">
              保存并刷新
            </n-button>
            <span v-if="authFailed" style="font-size: 11px; color: #d03050; line-height: 1.5">
              API 鉴权失败（401）：当前 token 与 server.token 不符或未设置。
              请在上框填入配置里的值；若配置里 token 为空则留空。
            </span>
          </n-space>
        </n-layout-sider>
        <n-layout-content
          :content-style="contentStyle"
          style="min-width: 0"
          :native-scrollbar="false"
        >
          <router-view />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>
