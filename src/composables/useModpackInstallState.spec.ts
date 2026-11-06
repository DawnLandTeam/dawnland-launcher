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

  describe('findRestorableDownloadsModpackTask (production restore logic)', () => {
    it('restores active untracked tasks and auto-tracks them', () => {
      const mockTasks = [
        { id: 'task-old-completed', status: 'Completed', task_type: { InstallModpack: { instance_name: 'PackA', is_update: false } } },
        { id: 'task-running', status: 'Running', task_type: { InstallModpack: { instance_name: 'PackB', is_update: false } } },
      ];

      const restored = state.findRestorableDownloadsModpackTask(mockTasks);
      expect(restored).not.toBeNull();
      expect(restored?.id).toBe('task-running');
      expect(state.downloadsModpackTaskId.value).toBe('task-running');
    });

    it('rejects untracked terminal tasks (Completed, Failed, Cancelled)', () => {
      const mockTasks = [
        { id: 'task-c', status: 'Completed', task_type: { InstallModpack: { instance_name: 'PackA', is_update: false } } },
        { id: 'task-f', status: 'Failed', task_type: { InstallModpack: { instance_name: 'PackB', is_update: false } } },
        { id: 'task-x', status: 'Cancelled', task_type: { InstallModpack: { instance_name: 'PackC', is_update: false } } },
      ];

      const restored = state.findRestorableDownloadsModpackTask(mockTasks);
      expect(restored).toBeNull();
      expect(state.downloadsModpackTaskId.value).toBeNull();
    });

    it('restores pre-scan candidate task even if it became terminal during scan', () => {
      // Simulate task that was active when restoration began, but transitioned to Completed while scan was in-flight
      const mockTasks = [
        { id: 'task-finishing', status: 'Completed', task_type: { InstallModpack: { instance_name: 'PackA', is_update: false } } },
      ];

      const restored = state.findRestorableDownloadsModpackTask(mockTasks, 'task-finishing');
      expect(restored).not.toBeNull();
      expect(restored?.id).toBe('task-finishing');
      expect(state.downloadsModpackTaskId.value).toBe('task-finishing');
    });

    it('restores tracked terminal tasks including Cancelled until dismissed', () => {
      state.setDownloadsModpackTaskId('task-cancelled');
      const mockTasks = [
        { id: 'task-cancelled', status: 'Cancelled', task_type: { InstallModpack: { instance_name: 'PackA', is_update: false } } },
      ];

      const restored = state.findRestorableDownloadsModpackTask(mockTasks);
      expect(restored).not.toBeNull();
      expect(restored?.id).toBe('task-cancelled');

      state.dismissTask('task-cancelled');
      expect(state.findRestorableDownloadsModpackTask(mockTasks)).toBeNull();
    });
  });

  describe('findRestorableInstanceUpdateTask (production update restore logic)', () => {
    it('restores active untracked update task matching instance name', () => {
      const mockTasks = [
        { id: 'task-up-diff', status: 'Running', task_type: { InstallModpack: { instance_name: 'OtherInstance', is_update: true } } },
        { id: 'task-up-target', status: 'Running', task_type: { InstallModpack: { instance_name: 'TargetInstance', is_update: true } } },
      ];

      const restored = state.findRestorableInstanceUpdateTask(mockTasks, 'TargetInstance');
      expect(restored).not.toBeNull();
      expect(restored?.id).toBe('task-up-target');
      expect(state.instanceUpdateTaskIds.value.TargetInstance).toBe('task-up-target');
    });

    it('restores tracked Cancelled update tasks until dismissed', () => {
      state.setInstanceUpdateTaskId('TargetInstance', 'task-up-cancelled');
      const mockTasks = [
        { id: 'task-up-cancelled', status: 'Cancelled', task_type: { InstallModpack: { instance_name: 'TargetInstance', is_update: true } } },
      ];

      expect(state.findRestorableInstanceUpdateTask(mockTasks, 'TargetInstance')).not.toBeNull();

      state.dismissTask('task-up-cancelled');
      expect(state.findRestorableInstanceUpdateTask(mockTasks, 'TargetInstance')).toBeNull();
    });
  });

  describe('parent view predicates (hasActiveDownloadsModpackTask & hasOngoingInstanceUpdateTask)', () => {
    it('treats tracked Cancelled tasks as eligible in parent view-selection predicates', () => {
      state.setDownloadsModpackTaskId('task-dl-canc');
      state.setInstanceUpdateTaskId('InstanceA', 'task-up-canc');

      const mockTasks = [
        { id: 'task-dl-canc', status: 'Cancelled', task_type: { InstallModpack: { instance_name: 'Inst1', is_update: false } } },
        { id: 'task-up-canc', status: 'Cancelled', task_type: { InstallModpack: { instance_name: 'InstanceA', is_update: true } } },
      ];

      expect(state.hasActiveDownloadsModpackTask(mockTasks)).toBe(true);
      expect(state.hasOngoingInstanceUpdateTask(mockTasks, 'InstanceA')).toBe(true);

      // Once dismissed, predicates must evaluate to false
      state.dismissTask('task-dl-canc');
      expect(state.hasActiveDownloadsModpackTask(mockTasks)).toBe(false);

      state.dismissTask('task-up-canc');
      expect(state.hasOngoingInstanceUpdateTask(mockTasks, 'InstanceA')).toBe(false);
    });

    it('returns false for untracked terminal tasks in parent view-selection predicates', () => {
      const mockTasks = [
        { id: 'task-old-1', status: 'Completed', task_type: { InstallModpack: { instance_name: 'Inst1', is_update: false } } },
        { id: 'task-old-2', status: 'Failed', task_type: { InstallModpack: { instance_name: 'InstanceA', is_update: true } } },
      ];

      expect(state.hasActiveDownloadsModpackTask(mockTasks)).toBe(false);
      expect(state.hasOngoingInstanceUpdateTask(mockTasks, 'InstanceA')).toBe(false);
    });
  });
});
