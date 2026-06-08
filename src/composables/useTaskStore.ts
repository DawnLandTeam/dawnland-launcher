import { ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { TaskState } from '../types/task';
import { getTaskName } from '../types/task';
import { notificationStore } from './useNotificationStore';
import i18n from '../i18n';

// Global state
const tasks = ref<TaskState[]>([]);
const isInitialized = ref(false);
const isTaskCenterOpen = ref(false);

export function useTaskStore() {
  // Init task history from backend
  async function init() {
    if (isInitialized.value) return;
    
    try {
      const history = await invoke<TaskState[]>('get_task_history');
      tasks.value = history;
      
      // Listen for updates
      await listen<TaskState>('task-progress-update', (event) => {
        const updatedTask = event.payload;
        const index = tasks.value.findIndex(t => t.id === updatedTask.id);
        
        if (index !== -1) {
          const oldStatus = tasks.value[index].status;
          tasks.value[index] = updatedTask;
          
          if (oldStatus !== updatedTask.status) {
            window.dispatchEvent(new CustomEvent('task-status-changed', { detail: updatedTask }));

            // Update terminal states with popup
            if (updatedTask.status === 'Failed') {
              notificationStore.updateNotification(updatedTask.id, {
                title: i18n.global.t('task.failedTitle', { name: getTaskName(updatedTask.task_type) }),
                description: updatedTask.error || i18n.global.t('task.failedDesc'),
                type: 'error',
                isPopup: true,
                duration: 5000,
                status: 'unread'
              });
            } else if (updatedTask.status === 'Completed') {
              notificationStore.updateNotification(updatedTask.id, {
                title: i18n.global.t('task.completedTitle', { name: getTaskName(updatedTask.task_type) }),
                description: i18n.global.t('task.completedDesc'),
                type: 'success',
                isPopup: true,
                duration: 3000,
                status: 'unread'
              });
            } else if (updatedTask.status === 'Cancelled') {
              notificationStore.updateNotification(updatedTask.id, {
                title: i18n.global.t('task.cancelledTitle', { name: getTaskName(updatedTask.task_type) }) || 'Task Cancelled',
                description: '',
                type: 'info',
                isPopup: true,
                duration: 3000,
                status: 'unread'
              });
            }
          } else if (updatedTask.status === 'Running') {
            // Silent update for running tasks
            notificationStore.updateNotification(updatedTask.id, {
              title: i18n.global.t('task.runningTitle', { name: getTaskName(updatedTask.task_type) }) || `Running: ${getTaskName(updatedTask.task_type)}`,
              description: updatedTask.progress?.detail || '',
              type: 'info',
              isPopup: false // Do not pop up repeatedly
            });
          }
        } else {
          tasks.value.unshift(updatedTask);
          window.dispatchEvent(new CustomEvent('task-added', { detail: updatedTask }));
          
          // New task started, show popup
          notificationStore.addNotification({
            id: updatedTask.id,
            title: i18n.global.t('task.startedTitle', { name: getTaskName(updatedTask.task_type) }) || `Task Started: ${getTaskName(updatedTask.task_type)}`,
            description: updatedTask.progress?.detail || '',
            type: 'info',
            isPopup: true,
            duration: 3000
          });
        }
      });

      // Listen for task deletions
      await listen<string>('task-deleted', (event) => {
        const deletedId = event.payload;
        tasks.value = tasks.value.filter(t => t.id !== deletedId);
      });
      
      isInitialized.value = true;
    } catch (e) {
      console.error('Failed to initialize task store:', e);
    }
  }

  const activeTasks = computed(() => {
    return tasks.value.filter(t => t.status === 'Pending' || t.status === 'Running' || t.status === 'Paused');
  });

  const hasActiveTasks = computed(() => activeTasks.value.length > 0);

  // Computed properties for UI stats
  const activeTaskCount = computed(() => activeTasks.value.length);

  const processingCancels = new Set<string>();

  async function cancelTask(id: string) {
    if (processingCancels.has(id)) return;
    
    try {
      processingCancels.add(id);
      await invoke('cancel_task', { taskId: id });
    } catch (e) {
      console.error('Failed to cancel task:', e);
    } finally {
      processingCancels.delete(id);
    }
  }

  async function clearHistory() {
    try {
      await invoke('clear_task_history');
      tasks.value = tasks.value.filter(t => t.status === 'Pending' || t.status === 'Running' || t.status === 'Paused');
    } catch (e) {
      console.error('Failed to clear task history:', e);
    }
  }

  const processingRetries = new Set<string>();

  async function retryTask(id: string) {
    if (processingRetries.has(id)) return;
    
    try {
      processingRetries.add(id);
      await invoke('retry_task', { taskId: id });
      // Remove the old task from frontend state so it gets replaced cleanly
      tasks.value = tasks.value.filter(t => t.id !== id);
    } catch (e) {
      console.error('Failed to retry task:', e);
    } finally {
      processingRetries.delete(id);
    }
  }

  function toggleTaskCenter() {
    isTaskCenterOpen.value = !isTaskCenterOpen.value;
  }

  return {
    tasks,
    activeTasks,
    hasActiveTasks,
    activeTaskCount,
    isTaskCenterOpen,
    init,
    cancelTask,
    retryTask,
    clearHistory,
    toggleTaskCenter,
  };
}
