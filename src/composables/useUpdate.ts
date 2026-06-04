import { ref, shallowRef } from 'vue';
import type { Update } from '@tauri-apps/plugin-updater';

// Global state for update
export const hasUpdateAvailable = ref(false);
export const globalUpdateInfo = shallowRef<Update | null>(null);

export function setUpdateAvailable(update: Update | null) {
  if (update) {
    hasUpdateAvailable.value = true;
    globalUpdateInfo.value = update;
  } else {
    hasUpdateAvailable.value = false;
    globalUpdateInfo.value = null;
  }
}
