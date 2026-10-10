<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  NAlert, NButton, NCard, NDivider, NEmpty, NForm, NFormItem, NInput, NSelect, NSpace, NTag,
  NInputNumber, NText, useMessage,
} from 'naive-ui'
import { api, type AccountInfo } from '../api'

const message = useMessage()
const router = useRouter()
const accounts = ref<AccountInfo[]>([])
const showCreate = ref(false)
const creating = ref(false)

const form = ref({
  key: '',
  display_name: '',
  server: 'official',
  account_name: '',
  uid: '',
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
    await api.patchAccount(a.key, { enabled: !a.enabled })
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

async function remove(a: AccountInfo): Promise<void> {
  if (!confirm(`删除账号 ${a.key}？（连同其 maa/mower bundle，不可恢复）`)) return
  try {
    await api.deleteAccount(a.key)
    message.success(`账号 ${a.key} 已删除`)
    await refresh()
  } catch (e) {
    message.error((e as Error).message)
  }
}

const uidEditing = ref<string | null>(null)
const uidDraft = ref('')

function startUidEdit(a: AccountInfo): void {
  uidEditing.value = a.key
  uidDraft.value = a.uid ?? ''
}

async function saveUid(a: AccountInfo): Promise<void> {
  const v = uidDraft.value.trim()
  if (v && !/^\d+$/.test(v)) {
    message.error('UID 须为纯数字')
    return
  }
  try {
    await api.patchAccount(a.key, { uid: v })
    message.success(v ? `账号 ${a.key} 的 UID 已设为 ${v}` : `账号 ${a.key} 的 UID 已清除（将跳过核验）`)
    uidEditing.value = null
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
      key: form.value.key.trim(),
      display_name: form.value.display_name || null,
      server: form.value.server,
      account_name: form.value.account_name,
      uid: form.value.uid.trim() || null,
      priority: form.value.priority,
      windows,
    })
    message.success(
      form.value.uid.trim()
        ? `账号 ${form.value.key} 已创建（下一步：CLI provision 完成人工登录）`
        : `账号 ${form.value.key} 已创建；未填 uid，切号后将跳过身份核验`,
    )
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
        <n-empty v-if="!accounts.length" description="暂无账号">
          <template #extra>
            <n-text depth="3" style="font-size: 12px">
              点右上「新建账号」创建，之后需 provision 人工登录一次
            </n-text>
          </template>
        </n-empty>
        <n-card
          v-for="a in accounts"
          :key="a.key"
          size="small"
          :bordered="true"
          style="min-width: 0"
        >
          <template #header>
            <n-space size="small" align="center" wrap>
              <n-tag size="small" :type="a.enabled ? 'success' : 'default'">
                {{ a.enabled ? '启用' : '停用' }}
              </n-tag>
              <n-text strong>{{ a.key }}</n-text>
              <n-text depth="3" style="font-size: 12px">
                {{ a.display_name }} · {{ a.server }}
              </n-text>
              <!-- uid 是核验依据，缺失标警告 -->
              <n-tag v-if="a.uid" size="small" type="info">UID {{ a.uid }}</n-tag>
              <n-tag v-else size="small" type="warning">无 UID · 跳过核验</n-tag>
              <n-tag v-for="d in a.provisioned_on" :key="d" size="small" type="success">
                已预置@{{ d }}
              </n-tag>
            </n-space>
          </template>
          <template #header-extra>
            <n-space size="small">
              <n-button size="tiny" type="primary" ghost @click="router.push(`/accounts/${a.key}`)">
                详情 / 任务
              </n-button>
              <n-button size="tiny" @click="toggle(a)">
                {{ a.enabled ? '停用' : '启用' }}
              </n-button>
              <n-button size="tiny" @click="uidEditing === a.key ? (uidEditing = null) : startUidEdit(a)">
                {{ a.uid ? '改 UID' : '补 UID' }}
              </n-button>
              <n-button size="tiny" type="error" @click="remove(a)">删除</n-button>
            </n-space>
          </template>

          <div
            style="
              display: flex; gap: 4px 16px; flex-wrap: wrap;
              font-size: 12px; color: gray;
            "
          >
            <span>切号匹配串：<b style="color: var(--n-text-color, #333)">{{ a.account_name }}</b></span>
            <span>优先级：{{ a.priority }}</span>
            <span v-if="a.slice">时间片：{{ a.slice }}</span>
            <span v-for="(w, i) in a.windows" :key="i">
              时间窗：{{ w.start }}–{{ w.end }} {{ w.executor }}{{ w.task ? `:${w.task}` : '' }}
            </span>
            <span v-if="!a.windows.length" style="color: var(--n-warning-color, #f0a020)">
              无时间窗 · 不参与自动调度
            </span>
          </div>

          <!-- UID 内联编辑：创建时漏填可在此补 -->
          <n-space
            v-if="uidEditing === a.key"
            size="small"
            align="center"
            style="margin-top: 8px"
          >
            <n-input
              v-model:value="uidDraft"
              size="small"
              placeholder="纯数字游戏 UID（清空则跳过核验）"
              style="width: 240px"
              clearable
              @keyup.enter="saveUid(a)"
            />
            <n-button size="small" type="primary" @click="saveUid(a)">保存</n-button>
            <n-button size="small" @click="uidEditing = null">取消</n-button>
          </n-space>
        </n-card>
      </n-space>
    </n-card>

    <n-card v-if="showCreate" title="新建账号" size="small">
      <n-form label-placement="top" :show-feedback="false">
        <!-- 基本信息：id/展示名/服务器/切号串 同一行组，窄屏自动换行 -->
        <div style="font-size: 12px; color: gray; margin: 4px 0 10px">
          身份标识 —— 三者用途不同，切号靠「切号匹配串」，核验靠「游戏 UID」
        </div>
        <div class="form-grid">
          <n-form-item
            label="账号 key（本地定位键）"
            path="key"
            :show-feedback="false"
          >
            <n-input v-model:value="form.key" placeholder="如 main → accounts/main/" />
            <template #feedback>
              <span style="font-size: 11px">
                本地定位键（非游戏身份）：决定目录名 accounts/&lt;key&gt;/ 与 CLI 参数，只含小写字母/数字/-/_
              </span>
            </template>
          </n-form-item>
          <n-form-item label="展示名" path="display_name" :show-feedback="false">
            <n-input v-model:value="form.display_name" placeholder="留空则与 id 相同" />
            <template #feedback>
              <span style="font-size: 11px">仅供控制台/日志显示，不参与切号</span>
            </template>
          </n-form-item>
          <n-form-item label="服务器" path="server">
            <n-select v-model:value="form.server" :options="serverOptions" />
          </n-form-item>
          <n-form-item label="切号匹配串" path="account_name" :show-feedback="false">
            <n-input
              v-model:value="form.account_name"
              :placeholder="form.server === 'official' ? '官服=打码手机号片段，如 123****8901' : 'B服=昵称'"
            />
            <template #feedback>
              <span style="font-size: 11px">
                MAA 快速登录列表的匹配依据，须在该设备已登录账号中唯一
              </span>
            </template>
          </n-form-item>
          <n-form-item label="游戏 UID（核验用）" path="uid" :show-feedback="false">
            <n-input
              v-model:value="form.uid"
              placeholder="纯数字，如 1000123456（可留空）"
              clearable
            />
            <template #feedback>
              <span style="font-size: 11px">
                切号后 OCR 核验登录身份，防登错号串数据；留空则跳过核验
              </span>
            </template>
          </n-form-item>
          <n-form-item label="队列优先级" path="priority" :show-feedback="false">
            <n-input-number
              v-model:value="form.priority"
              :min="0"
              :max="100"
              style="width: 100%"
            />
            <template #feedback>
              <span style="font-size: 11px">0-100，同一时刻多账号竞争时的抢占顺序</span>
            </template>
          </n-form-item>
        </div>

        <n-divider style="margin: 6px 0 14px" />

        <!-- 时间窗：起止时间相邻成组，执行器与任务跟随 -->
        <div style="font-size: 12px; color: gray; margin-bottom: 10px">
          调度时间窗（可留空 = 不限时段参与轮转）
        </div>
        <div class="form-grid">
          <n-form-item label="窗口开始" path="windowStart">
            <n-input v-model:value="form.windowStart" placeholder="08:00" />
          </n-form-item>
          <n-form-item label="窗口结束" path="windowEnd">
            <n-input v-model:value="form.windowEnd" placeholder="12:00" />
          </n-form-item>
          <n-form-item label="执行器" path="windowExecutor">
            <n-select
              v-model:value="form.windowExecutor"
              :options="executorOptions"
              :disabled="!form.windowStart || !form.windowEnd"
            />
          </n-form-item>
          <n-form-item v-if="form.windowExecutor === 'maa'" label="maa 任务名" path="windowTask">
            <n-input v-model:value="form.windowTask" placeholder="tasks/ 下的任务名" />
          </n-form-item>
        </div>

        <n-alert v-if="!form.windowStart || !form.windowEnd" type="warning" :bordered="false" style="margin-top: 4px">
          未填完整时间窗：该账号**不参与自动调度**（仅手动会话）。
          创建后可在「详情 / 任务」页补充时间窗——那里支持多行窗口与 maa 任务引用。
        </n-alert>

        <n-alert
          v-if="!form.uid.trim()"
          type="warning"
          :bordered="false"
          style="margin-top: 4px"
        >
          未填游戏 UID：切号后不做身份核验，若 MAA 匹配到错误账号不会被发现（doctor 也会告警串数据风险）。
          建议填写 —— 游戏内「个人名片」页可见。
        </n-alert>

        <n-space style="margin-top: 14px" align="center">
          <n-button type="primary" :loading="creating" @click="create">创建</n-button>
          <n-button @click="showCreate = false">取消</n-button>
          <n-text depth="3" style="font-size: 12px">
            创建后需在设备上人工登录一次（CLI：arkknight provision &lt;id&gt;）
          </n-text>
        </n-space>
      </n-form>
    </n-card>
  </n-space>
</template>

<style scoped>
/* 表单栅格：桌面 3 列，中屏 2 列，窄屏 1 列；min-width:0 防 grid 子项撑破容器 */
.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: 4px 16px;
  min-width: 0;
}
.form-grid :deep(.n-form-item) {
  min-width: 0;
}
</style>
