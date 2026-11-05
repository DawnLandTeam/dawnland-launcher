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
});
