<script setup lang="ts">
import { ref, shallowRef, onMounted } from "vue";
import { check } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainLayout from "./layouts/MainLayout.vue";
import UpdaterModal from "./components/UpdaterModal.vue";
import { setUpdateAvailable } from "./composables/useUpdate";

const isUpdateModalOpen = ref(false);
const updateInfo = shallowRef<Update | null>(null);

onMounted(() => {
  // Show window
  getCurrentWindow().show().catch(err => console.error("Failed to show window:", err));

  // Delay the update check slightly to ensure network and plugins are fully initialized
  setTimeout(() => {
    check().then(update => {
      if (update) {
        console.log(`Update available: ${update.version}`);
        updateInfo.value = update;
        isUpdateModalOpen.value = true;
        setUpdateAvailable(update);
      }
    }).catch(error => {
      console.error("Failed to check for updates on startup:", error);
    });
  }, 2000);
});
</script>

<template>
  <MainLayout />
  <UpdaterModal v-model:open="isUpdateModalOpen" :updateInfo="updateInfo" />
</template>