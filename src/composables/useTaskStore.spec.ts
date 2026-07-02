import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useTaskStore } from './useTaskStore';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { notificationStore } from './useNotificationStore';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn()
}));

vi.mock('../i18n', () => ({
  default: {
    global: {
      t: (key: string, args?: any) => args ? `${key} ${JSON.stringify(args)}` : key
    }
  }
}));

describe('useTaskStore composable', () => {
  let progressCallback: any;
  let deleteCallback: any;

  beforeEach(() => {
    vi.clearAllMocks();
    notificationStore.notifications.value = [];
  });

  it('initializes tasks and sets up listeners', async () => {
    const mockHistory = [{ id: '1', status: 'Pending', task_type: { Generic: { name: 'Test' } } }];
    vi.mocked(invoke).mockResolvedValueOnce(mockHistory);
    
    vi.mocked(listen).mockImplementation(async (event: string, cb: any) => {
      if (event === 'task-progress-update') progressCallback = cb;
      if (event === 'task-deleted') deleteCallback = cb;
      return () => {};
    });

    const store = useTaskStore();
    // Since it's a module, tasks might not be empty if tests run out of order, 
    // but vitest runs sequentially by default within a file.
    store.tasks.value = [];
    await store.init();

    expect(invoke).toHaveBeenCalledWith('get_task_history');
    expect(listen).toHaveBeenCalledWith('task-progress-update', expect.any(Function));
    expect(listen).toHaveBeenCalledWith('task-deleted', expect.any(Function));
    
    expect(store.tasks.value).toEqual(mockHistory);
    expect(store.activeTaskCount.value).toBe(1);
    
    expect(progressCallback).toBeDefined();
    expect(deleteCallback).toBeDefined();
  });

  it('updates task state and notifications when progress update event is fired', async () => {
    const store = useTaskStore();
    store.tasks.value = []; // Reset state

    // Fire new task event
    const newTask = { id: 'task-1', status: 'Running', task_type: { Generic: { name: 'Download' } } };
    progressCallback({ payload: newTask });

    expect(store.tasks.value[0]).toEqual(newTask);
    expect(notificationStore.notifications.value).toHaveLength(1);
    expect(notificationStore.notifications.value[0].title).toContain('task.startedTitle');

    // Fire task completed event
    const completedTask = { ...newTask, status: 'Completed' };
    progressCallback({ payload: completedTask });

    expect(store.tasks.value[0].status).toBe('Completed');
    expect(store.activeTasks.value).toHaveLength(0);
    // Notification store gets updated
    expect(notificationStore.notifications.value[0].title).toContain('task.completedTitle');
    expect(notificationStore.notifications.value[0].type).toBe('success');
  });

  it('handles task cancellation', async () => {
    const store = useTaskStore();
    await store.cancelTask('task-1');
    
    expect(invoke).toHaveBeenCalledWith('cancel_task', { taskId: 'task-1' });
  });

  it('removes tasks when task-deleted event fires', async () => {
    const store = useTaskStore();
    store.tasks.value = [{ id: 'task-1', status: 'Completed', task_type: { Generic: { name: 'Test' } } }] as any;
    
    expect(store.tasks.value).toHaveLength(1);
    
    // Simulate delete event
    deleteCallback({ payload: 'task-1' });
    
    expect(store.tasks.value).toHaveLength(0);
  });

  it('updates task state when task fails', async () => {
    const store = useTaskStore();
    store.tasks.value = [];
    
    // Add task
    progressCallback({ payload: { id: 'task-fail', status: 'Running', task_type: { Generic: { name: 'Fail' } } } });
    expect(store.tasks.value).toHaveLength(1);
    
    // Fail task
    progressCallback({ payload: { id: 'task-fail', status: 'Failed', error: 'some error', task_type: { Generic: { name: 'Fail' } } } });
    
    expect(store.tasks.value[0].status).toBe('Failed');
    expect(notificationStore.notifications.value[0].type).toBe('error');
    expect(notificationStore.notifications.value[0].title).toContain('task.failedTitle');
    expect(notificationStore.notifications.value[0].description).toBe('some error');
  });

  it('updates task state when task is cancelled', async () => {
    const store = useTaskStore();
    store.tasks.value = [];
    progressCallback({ payload: { id: 'task-cancel', status: 'Running', task_type: { Generic: { name: 'Cancel' } } } });
    progressCallback({ payload: { id: 'task-cancel', status: 'Cancelled', task_type: { Generic: { name: 'Cancel' } } } });
    
    expect(store.tasks.value[0].status).toBe('Cancelled');
    expect(notificationStore.notifications.value[0].type).toBe('info');
    expect(notificationStore.notifications.value[0].title).toContain('task.cancelledTitle');
  });

  it('updates silently when task is running', async () => {
    const store = useTaskStore();
    store.tasks.value = [];
    // First, add the task as Running (creates the task and started notification)
    progressCallback({ payload: { id: 'task-run', status: 'Running', task_type: { Generic: { name: 'Run' } } } });
    
    const notifLength = notificationStore.notifications.value.length;
    
    // Second, fire the progress update with Running again (triggers silent update)
    progressCallback({ payload: { id: 'task-run', status: 'Running', task_type: { Generic: { name: 'Run' } } } });
    expect(store.tasks.value[0].status).toBe('Running');
    expect(notificationStore.notifications.value).toHaveLength(notifLength);
    expect(notificationStore.notifications.value[0].title).toContain('task.runningTitle');
  });

  it('handles clearHistory', async () => {
    const store = useTaskStore();
    store.tasks.value = [
      { id: '1', status: 'Completed', task_type: { Generic: { name: '1' } } },
      { id: '2', status: 'Running', task_type: { Generic: { name: '2' } } }
    ] as any;
    
    await store.clearHistory();
    expect(invoke).toHaveBeenCalledWith('clear_task_history');
    expect(store.tasks.value).toHaveLength(1);
    expect(store.tasks.value[0].id).toBe('2');
  });

  it('handles retryTask', async () => {
    const store = useTaskStore();
    store.tasks.value = [{ id: 'task-retry', status: 'Failed', task_type: { Generic: { name: 'Retry' } } }] as any;
    
    await store.retryTask('task-retry');
    expect(invoke).toHaveBeenCalledWith('retry_task', { taskId: 'task-retry' });
    expect(store.tasks.value).toHaveLength(0);
  });

  it('handles UI toggles', async () => {
    const store = useTaskStore();
    
    store.isTaskDetailOpen.value = false;
    store.selectedTaskId.value = null;
    
    store.openTaskDetail('detail-1');
    expect(store.isTaskDetailOpen.value).toBe(true);
    expect(store.selectedTaskId.value).toBe('detail-1');
    
    store.closeTaskDetail();
    expect(store.isTaskDetailOpen.value).toBe(false);
    expect(store.selectedTaskId.value).toBe(null);
  });

  it('handles toggleTaskCenter', async () => {
    const store = useTaskStore();
    store.isTaskCenterOpen.value = false;
    store.isTaskDetailOpen.value = true;
    store.tasks.value = [
      { id: 'auto-clear-id', status: 'Completed', auto_clear: true, task_type: { Generic: { name: 'AutoClear' } } }
    ] as any;
    
    await store.toggleTaskCenter();
    expect(store.isTaskCenterOpen.value).toBe(true);
    
    await store.toggleTaskCenter();
    expect(store.isTaskCenterOpen.value).toBe(false);
    expect(store.isTaskDetailOpen.value).toBe(false);
    expect(invoke).toHaveBeenCalledWith('delete_task', { taskId: 'auto-clear-id' });
  });
});
