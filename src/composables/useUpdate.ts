import { ref, shallowRef } from 'vue';

export interface CustomUpdate {
  version: string;
  body: string;
}

// Global state for update
export const hasUpdateAvailable = ref(false);
export const globalUpdateInfo = shallowRef<CustomUpdate | null>(null);

export function setUpdateAvailable(update: CustomUpdate | null) {
  if (update) {
    hasUpdateAvailable.value = true;
    globalUpdateInfo.value = update;
  } else {
    hasUpdateAvailable.value = false;
    globalUpdateInfo.value = null;
  }
}
