<script setup lang="ts">
import { ref, shallowRef, onMounted } from "vue";
import { check } from "@tauri-apps/plugin-updater";
import type { Update } from "@tauri-apps/plugin-updater";
import { getCurrentWindow } from "@tauri-apps/api/window";
import MainLayout from "./layouts/MainLayout.vue";
import UpdaterModal from "./components/UpdaterModal.vue";

const isUpdateModalOpen = ref(false);
const updateInfo = shallowRef<Update | null>(null);

onMounted(async () => {
  try {
    await getCurrentWindow().show();
    const update = await check();
    if (update) {
      console.log(`Update available: ${update.version}`);
      updateInfo.value = update;
      isUpdateModalOpen.value = true;
    }
  } catch (error) {
    console.error("Failed to check for updates:", error);
  }
});
</script>

<template>
  <MainLayout />
  <UpdaterModal v-model:open="isUpdateModalOpen" :updateInfo="updateInfo" />
</template>