import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export interface InstanceStats {
  instanceId: string;
  launchCount: number;
  playTimeSeconds: number;
  lastPlayedAt: number | null;
}

export function useStatistics() {
  const allStats = ref<InstanceStats[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const fetchAllStats = async () => {
    loading.value = true;
    error.value = null;
    try {
      allStats.value = await invoke<InstanceStats[]>('get_all_stats');
    } catch (e: any) {
      console.error('Failed to fetch statistics:', e);
      error.value = e.message || String(e);
    } finally {
      loading.value = false;
    }
  };

  const getInstanceStats = async (instanceId: string): Promise<InstanceStats | null> => {
    try {
      return await invoke<InstanceStats | null>('get_instance_stats', { instanceId });
    } catch (e) {
      console.error(`Failed to fetch stats for ${instanceId}:`, e);
      return null;
    }
  };

  return {
    allStats,
    loading,
    error,
    fetchAllStats,
    getInstanceStats,
  };
}
