import { getTaskDetailRoute, type TaskState, type TaskTargetRoute } from '../types/task';
import { useModpackInstallState } from './useModpackInstallState';
import { useInstanceInstallState } from './useInstanceInstallState';

/**
 * Resolves the navigable task detail page route for a task using live install state.
 */
export function resolveTaskDetailRoute(task: Pick<TaskState, 'id' | 'status' | 'task_type'>): TaskTargetRoute | null {
  const modpackState = useModpackInstallState();
  const instanceState = useInstanceInstallState();

  return getTaskDetailRoute(task, {
    isModpackTaskDismissed: modpackState.isTaskDismissed,
    isTrackedDownloadsModpackTask: (taskId) => modpackState.downloadsModpackTaskId.value === taskId,
    isTrackedInstanceUpdateTask: (instanceName, taskId) =>
      modpackState.instanceUpdateTaskIds.value[instanceName] === taskId,
    isInstanceTaskDismissed: instanceState.isTaskDismissed,
    isTrackedInstanceInstallTask: (taskId) => instanceState.activeInstanceTaskId.value === taskId,
  });
}
