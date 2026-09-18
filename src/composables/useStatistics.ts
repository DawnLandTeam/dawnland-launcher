import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export interface InstanceStats {
  instanceId: string;
  launchCount: number;
  playTimeSeconds: number;
  lastPlayedAt: number | null;
}

const allStats = ref<InstanceStats[]>([]);
let globalFetchPromise: Promise<InstanceStats[]> | null = null;

export function useStatistics() {
  const loading = ref(false);
  const error = ref<string | null>(null);

  const fetchAllStats = async () => {
    loading.value = true;
    error.value = null;
    
    if (!globalFetchPromise) {
      globalFetchPromise = invoke<InstanceStats[]>('get_all_stats').finally(() => {
        globalFetchPromise = null;
      });
    }

    try {
      allStats.value = await globalFetchPromise;
    } catch (e: any) {
      console.error('Failed to fetch statistics:', e);
      error.value = e.message || String(e);
    } finally {
      loading.value = false;
    }
  };

  const fetchInstanceStats = async (instanceId: string): Promise<InstanceStats | null> => {
    try {
      return await invoke<InstanceStats | null>('get_instance_stats', { instanceId });
    } catch (e: any) {
      console.error(`Failed to fetch stats for ${instanceId}:`, e);
      throw new Error(e.message || String(e));
    }
  };

  const getInstanceStats = (instanceId: string): InstanceStats | undefined => {
    return allStats.value.find((s) => s.instanceId === instanceId);
  };

  const formatPlayTime = (seconds: number): string => {
    if (!seconds) return '0';
    const hours = seconds / 3600;
    return hours >= 10 ? Math.floor(hours).toString() : hours.toFixed(1);
  };

  return {
    allStats,
    loading,
    error,
    fetchAllStats,
    fetchInstanceStats,
    getInstanceStats,
    formatPlayTime,
  };
}
