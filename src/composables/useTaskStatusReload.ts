import { onMounted, onUnmounted } from 'vue';

export type TaskStatus = 'Completed' | 'Failed' | 'Cancelled' | string;

export function useTaskStatusReload(reloadFn: () => void) {
  const handleTaskStatusChanged = (e: Event) => {
    const customEvent = e as CustomEvent<{ status?: TaskStatus }>;
    const status = customEvent.detail?.status;
    if (status === 'Completed' || status === 'Failed' || status === 'Cancelled') {
      reloadFn();
    }
  };

  const handleInstancesChanged = () => {
    reloadFn();
  };

  onMounted(() => {
    window.addEventListener('task-status-changed', handleTaskStatusChanged);
    window.addEventListener('instances-changed', handleInstancesChanged);
  });

  onUnmounted(() => {
    window.removeEventListener('task-status-changed', handleTaskStatusChanged);
    window.removeEventListener('instances-changed', handleInstancesChanged);
  });
}
