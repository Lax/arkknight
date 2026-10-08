<script setup lang="ts">
import { NCard, NTag, NSpace, NText } from 'naive-ui'
</script>

<template>
  <n-space vertical size="large">
    <n-card title="总览">
      <n-space vertical>
        <n-text depth="3">
          akops —— 方舟多账号调度中心控制台骨架。页面与 API 对接属 M1 任务 8
          （设计文档 §13：账号/设备/会话/调度队列/统计/维护/深链 mower UI）。
        </n-text>
        <n-space>
          <n-tag type="info">M1 开发中</n-tag>
          <n-tag type="default">API: /api/status（待 server 实现）</n-tag>
        </n-space>
      </n-space>
    </n-card>
  </n-space>
</template>
