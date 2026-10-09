<script setup lang="ts">
import { onMounted, ref } from 'vue'
import {
  NButton, NCard, NForm, NFormItem, NInput, NSelect, NSpace, NTag, NInputNumber, useMessage,
} from 'naive-ui'
import { api, type AccountInfo } from '../api'

const message = useMessage()
const accounts = ref<AccountInfo[]>([])
const showCreate = ref(false)
const creating = ref(false)

const form = ref({
  id: '',
  display_name: '',
  server: 'official',
  account_name: '',
  priority: 50,
  windowStart: '',
  windowEnd: '',
  windowExecutor: 'mower',
  windowTask: '',
})

const serverOptions = [
  { label: '官服 (official)', value: 'official' },
  { label: 'B服 (bilibili)', value: 'bilibili' },
]
const executorOptions = [
  { label: 'mower（基建）', value: 'mower' },
  { label: 'maa（任务）', value: 'maa' },
]

async function refresh(): Promise<void> {
  try {
    accounts.value = (await api.accounts()) ?? []
  } catch (e) {
    message.error(`刷新失败：${(e as Error).message}`)
  }
}

async function toggle(a: AccountInfo): Promise<void> {
  try {
    await api.patchAccount(a.id, { enabled: !a.enabled })
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

async function remove(a: AccountInfo): Promise<void> {
  if (!confirm(`删除账号 ${a.id}？（连同其 maa/mower bundle，不可恢复）`)) return
  try {
    await api.deleteAccount(a.id)
    message.success(`账号 ${a.id} 已删除`)
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

async function create(): Promise<void> {
  creating.value = true
  try {
    const windows =
      form.value.windowStart && form.value.windowEnd
        ? [
            {
              start: form.value.windowStart,
              end: form.value.windowEnd,
              executor: form.value.windowExecutor,
              task: form.value.windowTask || null,
            },
          ]
        : []
    await api.createAccount({
      id: form.value.id,
      display_name: form.value.display_name || null,
      server: form.value.server,
      account_name: form.value.account_name,
      priority: form.value.priority,
      windows,
    })
    message.success(`账号 ${form.value.id} 已创建（下一步：CLI provision 完成人工登录）`)
    showCreate.value = false
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    creating.value = false
  }
}

onMounted(refresh)
</script>

<template>
  <n-space vertical size="large">
    <n-card title="账号管理" size="small">
      <template #header-extra>
        <n-button size="small" @click="showCreate = true">新建账号</n-button>
      </template>
      <n-space vertical>
        <n-tag v-if="!accounts.length" type="default">暂无账号</n-tag>
        <div
          v-for="a in accounts"
          :key="a.id"
          style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap"
        >
          <n-tag size="small" :type="a.enabled ? 'success' : 'default'">
            {{ a.enabled ? '启用' : '停用' }}
          </n-tag>
          <n-text strong>{{ a.id }}</n-text>
          <span style="color: gray">{{ a.display_name }} · {{ a.server }}</span>
          <span>{{ a.account_name }}</span>
          <span style="color: gray">priority={{ a.priority }}</span>
          <span v-for="(w, i) in a.windows" :key="i" style="color: gray; font-size: 12px">
            {{ w.start }}-{{ w.end }}:{{ w.executor }}{{ w.task ? `:${w.task}` : '' }}
          </span>
          <n-tag v-for="d in a.provisioned_on" :key="d" size="small" type="info">已预置@{{ d }}</n-tag>
          <n-button size="tiny" @click="toggle(a)">{{ a.enabled ? '停用' : '启用' }}</n-button>
          <n-button size="tiny" type="error" @click="remove(a)">删除</n-button>
        </div>
      </n-space>
    </n-card>

    <n-card v-if="showCreate" title="新建账号" size="small">
      <n-form inline>
        <n-form-item label="id（slug）"><n-input v-model:value="form.id" /></n-form-item>
        <n-form-item label="展示名"><n-input v-model:value="form.display_name" /></n-form-item>
        <n-form-item label="服务器"><n-select v-model:value="form.server" :options="serverOptions" style="width: 160px" /></n-form-item>
        <n-form-item label="切号串">
          <n-input v-model:value="form.account_name" placeholder="官服=打码手机号片段 / B服=昵称" style="width: 220px" />
        </n-form-item>
        <n-form-item label="优先级"><n-input-number v-model:value="form.priority" :min="0" :max="100" /></n-form-item>
        <n-form-item label="时间窗起"><n-input v-model:value="form.windowStart" placeholder="08:00（可空）" /></n-form-item>
        <n-form-item label="时间窗止"><n-input v-model:value="form.windowEnd" placeholder="12:00（可空）" /></n-form-item>
        <n-form-item label="执行器"><n-select v-model:value="form.windowExecutor" :options="executorOptions" style="width: 140px" /></n-form-item>
        <n-form-item v-if="form.windowExecutor === 'maa'" label="maa 任务">
          <n-input v-model:value="form.windowTask" placeholder="tasks/ 下的任务名" />
        </n-form-item>
      </n-form>
      <n-space>
        <n-button type="primary" :loading="creating" @click="create">创建</n-button>
        <n-button @click="showCreate = false">取消</n-button>
        <n-text depth="3">创建后需在设备上人工登录一次（CLI：arkreunion provision），切号串须全局唯一</n-text>
      </n-space>
    </n-card>
  </n-space>
</template>
