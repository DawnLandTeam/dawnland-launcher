import { ref } from 'vue';
import {
  isTaskActive,
  isTaskTerminal,
  isInstanceInstallTask,
  isDependencyInstanceInstallTask,
  type TaskState,
} from '../types/task';

type TaskSummary = Pick<TaskState, 'id' | 'status' | 'task_type'>;

// Track the active or most recent non-dependency instance install task in Downloads Center
const activeInstanceTaskId = ref<string | null>(null);

// Set of task IDs that the user has acknowledged or dismissed via "Finish" or "Back"
const dismissedTaskIds = ref<Set<string>>(new Set());

function isEligibleInstanceInstallTask(task: Pick<TaskState, 'task_type'>): boolean {
  return isInstanceInstallTask(task.task_type) && !isDependencyInstanceInstallTask(task.task_type);
}

/**
 * Manages shared instance-installation task restoration and dismissal state across views.
 */
export function useInstanceInstallState() {
  function setInstanceInstallTaskId(taskId: string | null) {
    activeInstanceTaskId.value = taskId;
  }

  function dismissTask(taskId: string) {
    dismissedTaskIds.value.add(taskId);
    if (activeInstanceTaskId.value === taskId) {
      activeInstanceTaskId.value = null;
    }
  }

  function isTaskDismissed(taskId: string): boolean {
    return dismissedTaskIds.value.has(taskId);
  }

  function clearState() {
    activeInstanceTaskId.value = null;
    dismissedTaskIds.value.clear();
  }

  function findActiveUntrackedInstanceInstallTask<T extends TaskSummary>(tasks: T[]): T | null {
    const found = tasks.find(
      (t) =>
        isEligibleInstanceInstallTask(t) &&
        !isTaskDismissed(t.id) &&
        isTaskActive(t.status)
    );
    return found || null;
  }

  function findRestorableInstanceInstallTask<T extends TaskSummary>(
    tasks: T[],
    candidateTaskId?: string | null
  ): T | null {
    const trackedId = candidateTaskId || activeInstanceTaskId.value;
    if (trackedId) {
      const task = tasks.find((t) => t.id === trackedId);
      if (
        task &&
        isEligibleInstanceInstallTask(task) &&
        !isTaskDismissed(task.id) &&
        (isTaskActive(task.status) || isTaskTerminal(task.status))
      ) {
        if (activeInstanceTaskId.value !== task.id) {
          setInstanceInstallTaskId(task.id);
        }
        return task;
      }
    }

    const untracked = findActiveUntrackedInstanceInstallTask(tasks);
    if (untracked) {
      setInstanceInstallTaskId(untracked.id);
      return untracked;
    }

    return null;
  }

  function hasActiveInstanceInstallTask<T extends TaskSummary>(tasks: T[]): boolean {
    if (activeInstanceTaskId.value) {
      const tracked = tasks.find((t) => t.id === activeInstanceTaskId.value);
      if (
        tracked &&
        isEligibleInstanceInstallTask(tracked) &&
        !isTaskDismissed(tracked.id) &&
        (isTaskActive(tracked.status) || isTaskTerminal(tracked.status))
      ) {
        return true;
      }
    }
    return Boolean(findActiveUntrackedInstanceInstallTask(tasks));
  }

  return {
    activeInstanceTaskId,
    setInstanceInstallTaskId,
    dismissTask,
    isTaskDismissed,
    clearState,
    findActiveUntrackedInstanceInstallTask,
    findRestorableInstanceInstallTask,
    hasActiveInstanceInstallTask,
  };
}
