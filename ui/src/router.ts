import { createRouter, createWebHistory } from 'vue-router'

// M1 任务 8 扩展全部页面（§13.1 页面清单）
const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'dashboard',
      component: () => import('./views/DashboardView.vue'),
    },
  ],
})

export default router
