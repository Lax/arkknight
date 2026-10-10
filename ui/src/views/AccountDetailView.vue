<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  NAlert, NButton, NCard, NDivider, NForm, NFormItem, NInput, NInputNumber,
  NSelect, NSpace, NTag, NText, NEmpty, useMessage,
} from 'naive-ui'
import { api, type AccountInfo } from '../api'

const route = useRoute()
const router = useRouter()
const message = useMessage()
const key = String(route.params.key ?? '')

const account = ref<AccountInfo | null>(null)
const loading = ref(true)

// ---- 基本信息 ----
const basic = ref({ display_name: '', server: 'official', account_name: '', uid: '' })
const savingBasic = ref(false)

// ---- 调度 ----
const sched = ref({
  priority: 50,
  slice: '', // 空串 = 全局默认
  runner: 'process',
  windows: [] as { start: string; end: string; executor: string; task: string }[],
})
const savingSched = ref(false)

const serverOptions = [
  { label: '官服 (official)', value: 'official' },
  { label: 'B服 (bilibili)', value: 'bilibili' },
]
const executorOptions = [
  { label: 'mower（基建排班）', value: 'mower' },
  { label: 'maa（任务）', value: 'maa' },
]
const runnerOptions = [
  { label: 'process（本地 Python 进程）', value: 'process' },
  { label: 'docker（会话容器，需镜像就绪）', value: 'docker' },
]

// ---- 任务：mower plan ----
const planContent = ref('')
const planExists = ref(false)
const planPath = ref('')
const planDirty = ref(false)
const savingPlan = ref(false)

// ---- 任务：maa tasks ----
const tasks = ref<{ name: string; size: number }[]>([])
const taskDir = ref('')
const activeTask = ref<string | null>(null)
const taskContent = ref('')
const taskDirty = ref(false)
const savingTask = ref(false)

async function load(): Promise<void> {
  loading.value = true
  try {
    const a = await api.account(key)
    account.value = a
    basic.value = {
      display_name: a.display_name,
      server: a.server,
      account_name: a.account_name,
      uid: a.uid ?? '',
    }
    sched.value = {
      priority: a.priority,
      slice: a.slice ?? '',
      runner: a.runner ?? 'process',
      windows: a.windows.map((w) => ({
        start: w.start,
        end: w.end,
        executor: w.executor,
        task: w.task ?? '',
      })),
    }
    const p = await api.getPlan(key)
    planExists.value = p.exists
    planPath.value = p.path
    planContent.value = p.content ?? ''
    planDirty.value = false
    const t = await api.maaTasks(key)
    tasks.value = t.tasks
    taskDir.value = t.dir
  } catch (e) {
    message.error(`加载失败：${(e as Error).message}`)
  } finally {
    loading.value = false
  }
}

async function saveBasic(): Promise<void> {
  if (!account.value) return
  savingBasic.value = true
  try {
    await api.patchAccount(key, {
      display_name: basic.value.display_name,
      server: basic.value.server,
      account_name: basic.value.account_name,
      uid: basic.value.uid.trim(),
    })
    message.success('基本信息已保存')
    await load()
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    savingBasic.value = false
  }
}

function addWindow(): void {
  sched.value.windows.push({ start: '', end: '', executor: 'mower', task: '' })
}

async function saveSchedule(): Promise<void> {
  savingSched.value = true
  try {
    const windows = sched.value.windows
      .filter((w) => w.start || w.end)
      .map((w) => ({
        start: w.start,
        end: w.end,
        executor: w.executor,
        task: w.executor === 'maa' ? w.task || null : null,
      }))
    await api.patchAccount(key, {
      priority: sched.value.priority,
      slice: sched.value.slice.trim() || null,
      runner: sched.value.runner,
      windows,
    })
    message.success(
      windows.length
        ? '调度配置已保存（下个调度周期生效）'
        : '已保存：时间窗为空，该账号不再参与自动调度（仅手动会话）',
    )
    await load()
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    savingSched.value = false
  }
}

async function loadTask(name: string): Promise<void> {
  try {
    const t = await api.maaTask(key, name)
    activeTask.value = name
    taskContent.value = t.content
    taskDirty.value = false
  } catch (e) {
    message.error((e as Error).message)
  }
}

async function saveTask(): Promise<void> {
  if (!activeTask.value) return
  savingTask.value = true
  try {
    await api.putMaaTask(key, activeTask.value, taskContent.value)
    message.success(`任务 ${activeTask.value} 已保存`)
    taskDirty.value = false
    const t = await api.maaTasks(key)
    tasks.value = t.tasks
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    savingTask.value = false
  }
}

async function removeTask(): Promise<void> {
  if (!activeTask.value) return
  if (!confirm(`删除任务 ${activeTask.value}？（不可恢复）`)) return
  try {
    await api.deleteMaaTask(key, activeTask.value)
    message.success(`任务 ${activeTask.value} 已删除`)
    activeTask.value = null
    taskContent.value = ''
    const t = await api.maaTasks(key)
    tasks.value = t.tasks
  } catch (e) {
    message.error((e as Error).message)
  }
}

function newTask(): void {
  const name = prompt('任务名（字母/数字/-/_，对应 accounts/<key>/maa/tasks/<名>.toml）')
  if (!name) return
  if (!/^[A-Za-z0-9_-]+$/.test(name)) {
    message.error('任务名仅允许字母/数字/-/_')
    return
  }
  activeTask.value = name
  taskContent.value = ''
  taskDirty.value = false
}

async function savePlan(): Promise<void> {
  savingPlan.value = true
  try {
    await api.putPlan(key, planContent.value)
    message.success('基建排班已保存（对运行中的会话不生效，下个时间片启用）')
    planExists.value = true
    planDirty.value = false
  } catch (e) {
    message.error((e as Error).message)
  } finally {
    savingPlan.value = false
  }
}

onMounted(load)
</script>

<template>
  <n-space vertical size="large">
    <n-space align="center" size="small">
      <n-button size="small" @click="router.push('/accounts')">← 账号列表</n-button>
      <n-text strong style="font-size: 16px">{{ key }}</n-text>
      <n-tag v-if="account" size="small" :type="account.enabled ? 'success' : 'default'">
        {{ account.enabled ? '启用' : '停用' }}
      </n-tag>
      <n-tag v-if="account?.uid" size="small" type="info">UID {{ account.uid }}</n-tag>
    </n-space>

    <n-alert v-if="account && !account.provisioned_on.length" type="warning" :bordered="false">
      该账号尚未在任何设备预置（人工登录一次）：自动调度会跳过它。
      在设备上登录后运行 <code>arkknight provision {{ key }} &lt;设备名&gt;</code> 完成记录。
    </n-alert>

    <!-- ===== 基本信息 ===== -->
    <n-card title="基本信息" size="small">
      <n-form label-placement="top" :show-feedback="false" style="max-width: 860px">
        <div class="form-grid">
          <n-form-item label="展示名">
            <n-input v-model:value="basic.display_name" />
          </n-form-item>
          <n-form-item label="服务器">
            <n-select v-model:value="basic.server" :options="serverOptions" />
          </n-form-item>
          <n-form-item label="切号匹配串">
            <n-input
              v-model:value="basic.account_name"
              :placeholder="basic.server === 'official' ? '官服=打码手机号片段' : 'B服=昵称'"
            />
            <template #feedback>
              <span style="font-size: 11px">MAA 快速登录列表的匹配依据，须在该设备已登录账号中唯一</span>
            </template>
          </n-form-item>
          <n-form-item label="游戏 UID（核验用）">
            <n-input v-model:value="basic.uid" placeholder="纯数字；清空 = 切号后跳过核验" />
          </n-form-item>
        </div>
        <n-space style="margin-top: 10px">
          <n-button type="primary" size="small" :loading="savingBasic" @click="saveBasic">
            保存基本信息
          </n-button>
        </n-space>
      </n-form>
    </n-card>

    <!-- ===== 调度 ===== -->
    <n-card title="调度" size="small">
      <template #header-extra>
        <n-text depth="3" style="font-size: 12px">
          时间窗为空 = 不参与自动调度（仅手动会话）
        </n-text>
      </template>
      <n-form label-placement="top" :show-feedback="false" style="max-width: 860px">
        <div class="form-grid">
          <n-form-item label="队列优先级">
            <n-input-number v-model:value="sched.priority" :min="0" :max="100" style="width: 100%" />
          </n-form-item>
          <n-form-item label="时间片（空 = 全局默认 2h）">
            <n-input v-model:value="sched.slice" placeholder="如 90m / 2h" />
          </n-form-item>
          <n-form-item label="mower 运行形态">
            <n-select v-model:value="sched.runner" :options="runnerOptions" />
          </n-form-item>
        </div>

        <n-divider style="margin: 10px 0" />
        <n-space align="center" style="margin-bottom: 8px">
          <n-text depth="3" style="font-size: 12px">
            每日时间窗（本地时区；到点切号执行对应任务，窗口须 start &lt; end）
          </n-text>
          <n-button size="tiny" @click="addWindow">+ 加一行</n-button>
        </n-space>
        <n-empty v-if="!sched.windows.length" description="无时间窗 —— 该账号不参与自动调度" size="small" />
        <div v-for="(w, i) in sched.windows" :key="i" class="window-row">
          <n-input v-model:value="w.start" placeholder="08:00" style="width: 90px" />
          <span style="color: gray">–</span>
          <n-input v-model:value="w.end" placeholder="12:00" style="width: 90px" />
          <n-select v-model:value="w.executor" :options="executorOptions" style="width: 180px" />
          <n-input
            v-if="w.executor === 'maa'"
            v-model:value="w.task"
            placeholder="maa 任务名（见下方任务列表）"
            style="flex: 1; min-width: 160px"
          />
          <n-button size="tiny" type="error" @click="sched.windows.splice(i, 1)">删</n-button>
        </div>

        <n-space style="margin-top: 12px">
          <n-button type="primary" size="small" :loading="savingSched" @click="saveSchedule">
            保存调度配置
          </n-button>
        </n-space>
      </n-form>
    </n-card>

    <!-- ===== 任务 ===== -->
    <n-card title="任务内容" size="small">
      <n-space vertical size="large">
        <!-- mower 基建排班 -->
        <div>
          <n-space align="center" style="margin-bottom: 6px">
            <n-text strong>mower 基建排班（plan.json）</n-text>
            <n-tag v-if="planExists" size="small" type="success">已配置</n-tag>
            <n-tag v-else size="small" type="warning">未创建 —— mower 会话将用 mower 默认排班</n-tag>
          </n-space>
          <n-input
            v-model:value="planContent"
            type="textarea"
            :autosize="{ minRows: 10, maxRows: 24 }"
            placeholder='{"default":"plan1","plan1":{"central":...},"conf":{...},"backup_plans":[]}'
            style="font-family: monospace; font-size: 12px"
            @update:value="planDirty = true"
          />
          <n-space style="margin-top: 8px" align="center">
            <n-button type="primary" size="small" :loading="savingPlan" @click="savePlan">
              保存排班
            </n-button>
            <n-text depth="3" style="font-size: 12px">
              保存前做 JSON 语法校验；完整 schema 由 mower 加载时校验（参考 mower 源码
              arknights_mower/utils/config/plan.py）。也可在 mower 会话 UI 里改，两边同源。
            </n-text>
          </n-space>
        </div>

        <n-divider style="margin: 4px 0" />

        <!-- maa 任务 -->
        <div>
          <n-space align="center" style="margin-bottom: 6px">
            <n-text strong>maa 自定义任务（时间窗 executor=maa 时引用）</n-text>
            <n-button size="tiny" @click="newTask">+ 新建</n-button>
          </n-space>
          <n-empty
            v-if="!tasks.length && !activeTask"
            description="暂无任务文件"
            size="small"
          >
            <template #extra>
              <n-text depth="3" style="font-size: 12px">目录：{{ taskDir }}</n-text>
            </template>
          </n-empty>
          <n-space v-if="tasks.length" size="small" style="margin-bottom: 8px" wrap>
            <n-button
              v-for="t in tasks"
              :key="t.name"
              size="tiny"
              :type="activeTask === t.name ? 'primary' : 'default'"
              @click="loadTask(t.name)"
            >
              {{ t.name }}
            </n-button>
          </n-space>
          <template v-if="activeTask">
            <n-input
              v-model:value="taskContent"
              type="textarea"
              :autosize="{ minRows: 8, maxRows: 24 }"
              :placeholder="`accounts/${key}/maa/tasks/${activeTask}.toml`"
              style="font-family: monospace; font-size: 12px"
              @update:value="taskDirty = true"
            />
            <n-space style="margin-top: 8px">
              <n-button type="primary" size="small" :loading="savingTask" @click="saveTask">
                保存任务
              </n-button>
              <n-button size="small" type="error" @click="removeTask">删除任务</n-button>
              <n-text depth="3" style="font-size: 12px">
                TOML 格式（MAA 任务模板）；调度窗口选 executor=maa 并填同名 task 即引用
              </n-text>
            </n-space>
          </template>
        </div>
      </n-space>
    </n-card>
  </n-space>
</template>

<style scoped>
.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
  gap: 4px 16px;
  min-width: 0;
}
.window-row {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 6px;
  flex-wrap: wrap;
}
</style>
