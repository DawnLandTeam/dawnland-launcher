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
});
