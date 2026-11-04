import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getErrorMessage } from '../utils/error';
import { applyCachedInstanceIcons } from './useInstanceIconUpdates';
import type { InstanceItem } from '../types';

const instances = ref<InstanceItem[]>([]);
const isLoaded = ref(false);

if (typeof window !== 'undefined') {
  window.addEventListener('instances-changed', () => {
    invoke<InstanceItem[]>('scan_installed_instances')
      .then((res) => {
        instances.value = applyCachedInstanceIcons(res);
      })
      .catch(console.error);
  });
}

export function useInstances() {
  const fetchInstances = async (force = false): Promise<InstanceItem[]> => {
    if (isLoaded.value && !force) {
      // Background refresh to keep data fresh without blocking UI
      invoke<InstanceItem[]>('scan_installed_instances').then((res) => {
        instances.value = applyCachedInstanceIcons(res);
      }).catch(console.error);
      return instances.value;
    }

    try {
      const res = await invoke<InstanceItem[]>('scan_installed_instances');
      instances.value = applyCachedInstanceIcons(res);
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
