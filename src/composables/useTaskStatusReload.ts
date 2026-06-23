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

  onMounted(() => {
    window.addEventListener('task-status-changed', handleTaskStatusChanged);
  });

  onUnmounted(() => {
    window.removeEventListener('task-status-changed', handleTaskStatusChanged);
  });
}
