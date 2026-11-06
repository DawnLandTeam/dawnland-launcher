import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getErrorMessage } from '../utils/error';
import { applyCachedInstanceIcons } from './useInstanceIconUpdates';
import type { InstanceItem } from '../types';

const instances = ref<InstanceItem[]>([]);
const isLoaded = ref(false);
let activeScanPromise: Promise<InstanceItem[]> | null = null;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

async function executeScan(): Promise<InstanceItem[]> {
  if (activeScanPromise) {
    return activeScanPromise;
  }
  activeScanPromise = (async () => {
    try {
      const res = await invoke<InstanceItem[]>('scan_installed_instances');
      instances.value = applyCachedInstanceIcons(res);
      isLoaded.value = true;
      return instances.value;
    } finally {
      activeScanPromise = null;
    }
  })();
  return activeScanPromise;
}

if (typeof window !== 'undefined') {
  window.addEventListener('instances-changed', () => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      executeScan().catch(console.error);
    }, 250);
  });
}

export function useInstances() {
  const fetchInstances = async (force = false): Promise<InstanceItem[]> => {
    if (isLoaded.value && !force) {
      // Background refresh to keep data fresh without blocking UI
      executeScan().catch(console.error);
      return instances.value;
    }

    try {
      await executeScan();
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
