import { describe, it, expect, beforeEach } from 'vitest';
import { useInstanceInstallState } from './useInstanceInstallState';
import type { TaskState } from '../types/task';

type TaskSummary = Pick<TaskState, 'id' | 'status' | 'task_type'>;

describe('useInstanceInstallState', () => {
  const state = useInstanceInstallState();

  beforeEach(() => {
    state.clearState();
  });

  it('tracks, dismisses, and cleans up activeInstanceTaskId', () => {
    expect(state.activeInstanceTaskId.value).toBeNull();
    state.setInstanceInstallTaskId('task-inst-1');
    expect(state.activeInstanceTaskId.value).toBe('task-inst-1');
    expect(state.isTaskDismissed('task-inst-1')).toBe(false);

    state.dismissTask('task-inst-1');
    expect(state.isTaskDismissed('task-inst-1')).toBe(true);
    expect(state.activeInstanceTaskId.value).toBeNull();
  });

  it('restores active untracked non-dependency instance tasks and ignores dependency tasks', () => {
    const mockTasks: TaskSummary[] = [
      {
        id: 'dep-vanilla',
        status: 'Running',
        task_type: { InstallVanilla: { version_id: '1.21.4', version_json_url: 'https://example.com', is_dependency: true } },
      },
      {
        id: 'old-completed',
        status: 'Completed',
        task_type: { InstallFabric: { mc_version: '1.21.4', fabric_version: '0.16.10', custom_instance_name: 'Fab' } },
      },
      {
        id: 'active-fabric',
        status: 'Running',
        task_type: { InstallFabric: { mc_version: '1.21.4', fabric_version: '0.16.10', custom_instance_name: 'Fab2' } },
      },
    ];

    const restored = state.findRestorableInstanceInstallTask(mockTasks);
    expect(restored?.id).toBe('active-fabric');
    expect(state.activeInstanceTaskId.value).toBe('active-fabric');
  });

  it('rejects untracked terminal tasks and respects dismissal', () => {
    const mockTasks: TaskSummary[] = [
      {
        id: 'old-done',
        status: 'Completed',
        task_type: { InstallVanilla: { version_id: '1.20.1', version_json_url: 'https://example.com' } },
      },
    ];

    expect(state.findRestorableInstanceInstallTask(mockTasks)).toBeNull();
    expect(state.hasActiveInstanceInstallTask(mockTasks)).toBe(false);

    state.setInstanceInstallTaskId('old-done');
    expect(state.hasActiveInstanceInstallTask(mockTasks)).toBe(true);
    expect(state.findRestorableInstanceInstallTask(mockTasks)?.id).toBe('old-done');

    state.dismissTask('old-done');
    expect(state.hasActiveInstanceInstallTask(mockTasks)).toBe(false);
    expect(state.findRestorableInstanceInstallTask(mockTasks)).toBeNull();
  });

  it('restores a candidateTaskId from navigation even if it finished in-flight', () => {
    const mockTasks: TaskSummary[] = [
      {
        id: 'cand-1',
        status: 'Completed',
        task_type: { InstallForge: { mc_version: '1.20.1', loader_version: '47.3.0', loader_type: 'neoforge', custom_instance_name: 'Neo' } },
      },
    ];

    const restored = state.findRestorableInstanceInstallTask(mockTasks, 'cand-1');
    expect(restored?.id).toBe('cand-1');
    expect(state.activeInstanceTaskId.value).toBe('cand-1');
  });
});
