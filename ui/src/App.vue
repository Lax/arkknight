<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { NConfigProvider, NMessageProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, zhCN, dateZhCN } from 'naive-ui'
import type { MenuOption } from 'naive-ui'
import { h } from 'vue'
import { RouterLink } from 'vue-router'

const route = useRoute()

// M1 任务 8 的页面清单（设计文档 §13.1）；骨架先占位路由
const menuOptions: MenuOption[] = [
  { label: () => h(RouterLink, { to: '/' }, { default: () => '总览' }), key: 'dashboard' },
  { label: '账号管理（M1 任务 8）', key: 'accounts', disabled: true },
  { label: '设备管理（M1 任务 8）', key: 'devices', disabled: true },
  { label: '会话（M1 任务 8）', key: 'sessions', disabled: true },
  { label: '维护中心（M1 任务 8）', key: 'maintenance', disabled: true },
]

const activeKey = computed(() => (route.name === 'dashboard' ? 'dashboard' : ''))
</script>

<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-layout has-sider style="height: 100vh">
        <n-layout-sider bordered content-style="padding: 12px" :width="220">
          <h2 style="margin: 4px 8px 16px">akops</h2>
          <n-menu :options="menuOptions" :value="activeKey" />
        </n-layout-sider>
        <n-layout-content content-style="padding: 24px">
          <router-view />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>
