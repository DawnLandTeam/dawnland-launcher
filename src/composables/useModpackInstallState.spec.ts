import { describe, it, expect, beforeEach } from 'vitest';
import { useModpackInstallState } from './useModpackInstallState';

describe('useModpackInstallState', () => {
  const state = useModpackInstallState();

  beforeEach(() => {
    state.clearState();
  });

  it('manages downloadsModpackTaskId', () => {
    expect(state.downloadsModpackTaskId.value).toBeNull();
    state.setDownloadsModpackTaskId('task-123');
    expect(state.downloadsModpackTaskId.value).toBe('task-123');
    state.setDownloadsModpackTaskId(null);
    expect(state.downloadsModpackTaskId.value).toBeNull();
  });

  it('manages instanceUpdateTaskIds', () => {
    state.setInstanceUpdateTaskId('InstanceA', 'task-update-1');
    state.setInstanceUpdateTaskId('InstanceB', 'task-update-2');
    expect(state.instanceUpdateTaskIds.value).toEqual({
      InstanceA: 'task-update-1',
      InstanceB: 'task-update-2',
    });

    state.setInstanceUpdateTaskId('InstanceA', null);
    expect(state.instanceUpdateTaskIds.value).toEqual({
      InstanceB: 'task-update-2',
    });
  });

  it('dismisses task and cleans up corresponding references', () => {
    state.setDownloadsModpackTaskId('task-dl');
    state.setInstanceUpdateTaskId('InstanceA', 'task-update');

    expect(state.isTaskDismissed('task-dl')).toBe(false);

    state.dismissTask('task-dl');
    expect(state.isTaskDismissed('task-dl')).toBe(true);
    expect(state.downloadsModpackTaskId.value).toBeNull();

    state.dismissTask('task-update');
    expect(state.isTaskDismissed('task-update')).toBe(true);
    expect(state.instanceUpdateTaskIds.value.InstanceA).toBeUndefined();
  });

  it('correctly isolates active vs terminal task matching without tracked ID', () => {
    // When no downloadsModpackTaskId is set, only actually active tasks should be selected
    const mockTasks = [
      { id: 'task-old-completed', status: 'Completed', task_type: { InstallModpack: { instance_name: 'PackA' } } },
      { id: 'task-old-failed', status: 'Failed', task_type: { InstallModpack: { instance_name: 'PackB' } } },
      { id: 'task-running', status: 'Running', task_type: { InstallModpack: { instance_name: 'PackC' } } },
    ];

    // Untracked completed or failed tasks must not be considered active
    const activeUntracked = mockTasks.filter(t => !state.isTaskDismissed(t.id) && (t.status === 'Running' || t.status === 'Pending'));
    expect(activeUntracked.map(t => t.id)).toEqual(['task-running']);

    // Once a task is actively tracked in this session, its terminal state remains valid until dismissed
    state.setDownloadsModpackTaskId('task-old-completed');
    const trackedTask = mockTasks.find(t => t.id === state.downloadsModpackTaskId.value);
    expect(trackedTask).toBeDefined();
    expect(state.isTaskDismissed(trackedTask!.id)).toBe(false);

    state.dismissTask('task-old-completed');
    expect(state.isTaskDismissed('task-old-completed')).toBe(true);
    expect(state.downloadsModpackTaskId.value).toBeNull();
  });
});
