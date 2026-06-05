<script setup lang="ts">
import { ref, shallowRef, onMounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainLayout from "./layouts/MainLayout.vue";
import UpdaterModal from "./components/UpdaterModal.vue";
import { setUpdateAvailable, type CustomUpdate } from "./composables/useUpdate";
import { getVersion } from "@tauri-apps/api/app";

const isUpdateModalOpen = ref(false);
const updateInfo = shallowRef<CustomUpdate | null>(null);

onMounted(() => {
  // Show window
  getCurrentWindow().show().catch(err => console.error("Failed to show window:", err));

  // Delay the update check slightly to ensure network and plugins are fully initialized
  setTimeout(async () => {
    try {
      const currentVersion = await getVersion();
      const targetOS = navigator.userAgent.includes("Windows") ? "windows-standalone" : "linux-standalone";
      const res = await fetch(`https://api.dawnland.cn/api/launcher/update/${targetOS}/${currentVersion}`);
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
});
</script>

<template>
  <MainLayout />
  <UpdaterModal v-model:open="isUpdateModalOpen" :updateInfo="updateInfo" />
</template>