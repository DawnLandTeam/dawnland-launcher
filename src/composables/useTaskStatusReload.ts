import { onMounted, onUnmounted } from 'vue';

export type TaskStatus = 'Completed' | 'Failed' | 'Cancelled' | string;

export function useTaskStatusReload(reloadFn: () => void | Promise<void>) {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const triggerDebouncedReload = () => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      reloadFn();
    }, 250);
  };

  const handleTaskStatusChanged = (e: Event) => {
    const customEvent = e as CustomEvent<{ status?: TaskStatus }>;
    const status = customEvent.detail?.status;
    if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
      triggerDebouncedReload();
    }
  };

  const handleInstancesChanged = () => {
    triggerDebouncedReload();
  };

  onMounted(() => {
    window.addEventListener('task-status-changed', handleTaskStatusChanged);
    window.addEventListener('instances-changed', handleInstancesChanged);
  });

  onUnmounted(() => {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    window.removeEventListener('task-status-changed', handleTaskStatusChanged);
    window.removeEventListener('instances-changed', handleInstancesChanged);
  });
}
