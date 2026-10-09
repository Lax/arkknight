<script setup lang="ts">
import { h, ref } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import {
  NConfigProvider, NDateLocale, NLayout, NLayoutContent, NLayoutSider, NMenu, NMessageProvider,
  NInput, NSpace, zhCN, dateZhCN,
} from 'naive-ui'
import type { MenuOption } from 'naive-ui'
import { getToken, setToken } from './api'

const route = useRoute()
const tokenInput = ref(getToken())

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
  location.reload()
}
</script>

<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-layout has-sider style="height: 100vh">
        <n-layout-sider bordered content-style="padding: 12px" :width="210">
          <h2 style="margin: 4px 8px 14px">arkreunion 控制台</h2>
          <n-menu :options="menuOptions" :value="String(route.name)" />
          <n-space vertical style="margin-top: 18px; padding: 0 8px" size="small">
            <span style="font-size: 12px; color: gray">API token</span>
            <n-input
              v-model:value="tokenInput"
              size="tiny"
              type="password"
              show-password-on="click"
              placeholder="未设可留空"
              @keyup.enter="saveToken"
            />
          </n-space>
        </n-layout-sider>
        <n-layout-content content-style="padding: 20px">
          <router-view />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>
