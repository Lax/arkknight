import { createRouter, createWebHistory } from 'vue-router'

// M1 v1 页面清单（§13.1）；统计页（ECharts）与策略编辑器属任务 8 尾批
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', name: 'dashboard', component: () => import('./views/DashboardView.vue') },
    { path: '/sessions', name: 'sessions', component: () => import('./views/SessionsView.vue') },
    { path: '/logs', name: 'logs', component: () => import('./views/LogsView.vue') },
    { path: '/accounts', name: 'accounts', component: () => import('./views/AccountsView.vue') },
    { path: '/devices', name: 'devices', component: () => import('./views/DevicesView.vue') },
    { path: '/schedule', name: 'schedule', component: () => import('./views/ScheduleView.vue') },
    { path: '/doctor', name: 'doctor', component: () => import('./views/DoctorView.vue') },
  ],
})

export default router
