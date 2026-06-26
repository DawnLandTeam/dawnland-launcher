import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getErrorMessage } from '../utils/error';

const instances = ref<any[]>([]);
const isLoaded = ref(false);

export function useInstances() {
  const fetchInstances = async (force = false) => {
    if (isLoaded.value && !force) {
      // Background refresh to keep data fresh without blocking UI
      invoke('scan_installed_instances').then((res: any) => {
        instances.value = res;
      }).catch(console.error);
      return instances.value;
    }

    try {
      const res: any[] = await invoke('scan_installed_instances');
      instances.value = res;
      isLoaded.value = true;
    } catch (e) {
      console.error('Failed to load instances:', getErrorMessage(e));
    }
    return instances.value;
  };

  return {
    instances,
    isLoaded,
    fetchInstances
  };
}
