import { createRouter, createWebHashHistory } from "vue-router";
import ServersPage from "../pages/ServersPage.vue";
import SettingsPage from "../pages/SettingsPage.vue";
import { useServerStore } from "../stores/server";

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", redirect: "/servers" },
    { path: "/servers", component: ServersPage, meta: { standalone: true } },
    { path: "/settings", component: SettingsPage, meta: { standalone: true } },
    {
      path: "/s/:id",
      component: () => import("../pages/DashboardPage.vue"),
      props: true,
    },
    {
      path: "/s/:id/gpu",
      component: () => import("../pages/GpuPage.vue"),
      props: true,
    },
    {
      path: "/s/:id/init",
      component: () => import("../pages/InitPage.vue"),
      props: true,
    },
    {
      path: "/s/:id/frameworks",
      component: () => import("../pages/FrameworksPage.vue"),
      props: true,
    },
    {
      path: "/s/:id/models",
      component: () => import("../pages/ModelsPage.vue"),
      props: true,
    },
  ],
});

router.beforeEach((to) => {
  const store = useServerStore();
  if (to.meta.standalone) {
    if (store.currentId && to.path !== "/settings") return `/s/${store.currentId}`;
    return true;
  }
  if (!store.currentId) return "/servers";
  if (to.params.id !== store.currentId) return `/s/${store.currentId}`;
  return true;
});

export default router;
