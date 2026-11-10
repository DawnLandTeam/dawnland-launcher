import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { getErrorMessage } from '../utils/error';
import { applyCachedInstanceIcons } from './useInstanceIconUpdates';
import type { InstanceItem } from '../types';

const instances = ref<InstanceItem[]>([]);
const isLoaded = ref(false);
let activeScanPromise: Promise<InstanceItem[]> | null = null;
let queuedScanPromise: Promise<InstanceItem[]> | null = null;
let lastScanTime = 0;
const MIN_SCAN_INTERVAL_MS = 150;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;

async function executeScan(force = false): Promise<InstanceItem[]> {
  if (activeScanPromise) {
    if (force) {
      if (!queuedScanPromise) {
        queuedScanPromise = activeScanPromise
          .catch(() => {})
          .then(() => {
            queuedScanPromise = null;
            return executeScan(true);
          });
      }
      return queuedScanPromise;
    }
    return activeScanPromise;
  }
  const now = Date.now();
  if (!force && isLoaded.value && now - lastScanTime < MIN_SCAN_INTERVAL_MS) {
    return instances.value;
  }
  activeScanPromise = (async () => {
    try {
      const res = await invoke<InstanceItem[]>('scan_installed_instances');
      instances.value = applyCachedInstanceIcons(res);
      isLoaded.value = true;
      lastScanTime = Date.now();
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
      // Return cached instances immediately, and do a debounced/throttled background refresh
      const now = Date.now();
      if (now - lastScanTime >= MIN_SCAN_INTERVAL_MS) {
        executeScan(false).catch(console.error);
      }
      return instances.value;
    }

    try {
      await executeScan(force);
    } catch (e) {
      console.error('Failed to load instances:', getErrorMessage(e));
    }
    return instances.value;
  };

  return {
    instances,
    isLoaded,
    fetchInstances,
    loadInstances: fetchInstances,
    scanInstances: () => executeScan(true),
  };
}

export function _resetStateForTesting() {
  instances.value = [];
  isLoaded.value = false;
  activeScanPromise = null;
  queuedScanPromise = null;
  lastScanTime = 0;
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
}
