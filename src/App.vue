<script setup lang="ts">
import { ref, shallowRef, onMounted, onUnmounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainLayout from "./layouts/MainLayout.vue";
import UpdaterModal from "./components/UpdaterModal.vue";
import { setUpdateAvailable, type CustomUpdate } from "./composables/useUpdate";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "vue-i18n";
import { useTaskStore } from "./composables/useTaskStore";
import TaskCenter from "./components/TaskCenter.vue";
import Toaster from "./components/Toaster.vue";

const isUpdateModalOpen = ref(false);
const updateInfo = shallowRef<CustomUpdate | null>(null);
const { locale, t } = useI18n();
const taskStore = useTaskStore();

onMounted(async () => {
  // Initialize task center
  await taskStore.init();

  // Show window
  getCurrentWindow().show().catch(err => console.error("Failed to show window:", err));

  // Async precise locale detection from Rust
  if (localStorage.getItem('userSelectedLanguage') !== 'true') {
    try {
      const sysLocale = await invoke<string | null>("get_system_locale");
      if (sysLocale) {
        const detected = sysLocale.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
        if (locale.value !== detected) {
          locale.value = detected;
          localStorage.setItem('language', detected);
        }
      }
    } catch (e) {
      console.warn("Failed to get system locale from Rust:", e);
    }
  }

  // Delay the update check slightly to ensure network and plugins are fully initialized
  setTimeout(async () => {
    try {
      const currentVersion = await getVersion();
      const targetOS = navigator.userAgent.includes("Windows") ? "windows-standalone" : "linux-standalone";
      const baseUrl = import.meta.env.VITE_WEB_BACKEND_URL || 'http://localhost:3030';
      const res = await fetch(`${baseUrl}/api/launcher/update/${targetOS}/${currentVersion}`);
      if (res.status === 200) {
        const data = await res.json();
        if (data.version && data.version !== currentVersion) {
          console.log(`Update available: ${data.version}`);
          const update = { version: data.version, body: data.notes || '' };
          updateInfo.value = update;
          isUpdateModalOpen.value = true;
          setUpdateAvailable(update);
        }
      }
    } catch (error) {
      console.error("Failed to check for updates on startup:", error);
    }
  }, 2000);

  document.addEventListener('dragenter', handleDrag, true);
  document.addEventListener('dragover', handleDrag, true);
  document.addEventListener('drop', handleDrop, true);
});

const handleDrag = (e: DragEvent) => {
  e.preventDefault();
  e.stopPropagation();
  if (e.dataTransfer) {
    e.dataTransfer.dropEffect = 'copy';
  }
};

const handleDrop = async (e: DragEvent) => {
  e.preventDefault();
  e.stopPropagation();
  
  let text = e.dataTransfer?.getData('text/plain');
  if (!text) text = e.dataTransfer?.getData('text/html');
  if (!text) text = e.dataTransfer?.getData('text/uri-list');

  if (text) {
    // Regex search to find the authlib string even if it's wrapped in HTML or other text
    const match = text.match(/authlib-injector:yggdrasil-server:([^\s"']+)/);
    if (match) {
      const url = decodeURIComponent(match[1]);
      try {
        await invoke("add_authlib_server", { url: url.trim() });
        window.dispatchEvent(new CustomEvent('authlib-servers-updated'));
        alert(t('settings.authlib.addSuccess', { url }));
      } catch (err) {
        alert(t('settings.authlib.addFailed', { error: String(err) }));
      }
    }
  }
};

onUnmounted(() => {
  document.removeEventListener('dragenter', handleDrag, true);
  document.removeEventListener('dragover', handleDrag, true);
  document.removeEventListener('drop', handleDrop, true);
});
</script>

<template>
  <MainLayout />
  <UpdaterModal v-model:open="isUpdateModalOpen" :updateInfo="updateInfo" />
  <TaskCenter />
  <Toaster />
</template>