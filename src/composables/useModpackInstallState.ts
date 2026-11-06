import { ref } from 'vue';
import {
  isTaskActive,
  isTaskTerminal,
  isModpackTask,
  isModpackUpdateTask,
  getModpackTaskInstanceName,
} from '../types/task';

// Track the active or most recent modpack install task in Downloads Center
const downloadsModpackTaskId = ref<string | null>(null);

// Track instance update task IDs: instanceName -> taskId
const instanceUpdateTaskIds = ref<Record<string, string>>({});

// Set of task IDs that the user has acknowledged or dismissed via "Finish" or "Back"
const dismissedTaskIds = ref<Set<string>>(new Set());

export function useModpackInstallState() {
  function setDownloadsModpackTaskId(taskId: string | null) {
    downloadsModpackTaskId.value = taskId;
  }

  function setInstanceUpdateTaskId(instanceName: string, taskId: string | null) {
    if (taskId) {
      instanceUpdateTaskIds.value = {
        ...instanceUpdateTaskIds.value,
        [instanceName]: taskId,
      };
    } else {
      const copy = { ...instanceUpdateTaskIds.value };
      delete copy[instanceName];
      instanceUpdateTaskIds.value = copy;
    }
  }

  function dismissTask(taskId: string) {
    dismissedTaskIds.value.add(taskId);
    if (downloadsModpackTaskId.value === taskId) {
      downloadsModpackTaskId.value = null;
    }
    for (const [key, val] of Object.entries(instanceUpdateTaskIds.value)) {
      if (val === taskId) {
        const copy = { ...instanceUpdateTaskIds.value };
        delete copy[key];
        instanceUpdateTaskIds.value = copy;
      }
    }
  }

  function isTaskDismissed(taskId: string): boolean {
    return dismissedTaskIds.value.has(taskId);
  }

  function clearState() {
    downloadsModpackTaskId.value = null;
    instanceUpdateTaskIds.value = {};
    dismissedTaskIds.value.clear();
  }

  function findActiveUntrackedDownloadsModpackTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[]
  ): T | null {
    const found = tasks.find(
      (t) =>
        isModpackTask(t.task_type) &&
        !isModpackUpdateTask(t.task_type) &&
        !isTaskDismissed(t.id) &&
        isTaskActive(t.status)
    );
    return found || null;
  }

  function findActiveUntrackedInstanceUpdateTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[],
    instanceName: string
  ): T | null {
    const found = tasks.find(
      (t) =>
        isModpackTask(t.task_type) &&
        isModpackUpdateTask(t.task_type) &&
        getModpackTaskInstanceName(t.task_type) === instanceName &&
        !isTaskDismissed(t.id) &&
        isTaskActive(t.status)
    );
    return found || null;
  }

  function findRestorableDownloadsModpackTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[],
    candidateTaskId?: string | null
  ): T | null {
    const trackedId = downloadsModpackTaskId.value || candidateTaskId;
    if (trackedId) {
      const task = tasks.find((t) => t.id === trackedId);
      if (
        task &&
        !isModpackUpdateTask(task.task_type) &&
        !isTaskDismissed(task.id) &&
        (isTaskActive(task.status) || isTaskTerminal(task.status))
      ) {
        if (downloadsModpackTaskId.value !== task.id) {
          setDownloadsModpackTaskId(task.id);
        }
        return task;
      }
    }

    const untracked = findActiveUntrackedDownloadsModpackTask(tasks);
    if (untracked) {
      setDownloadsModpackTaskId(untracked.id);
      return untracked;
    }

    return null;
  }

  function findRestorableInstanceUpdateTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[],
    instanceName: string,
    candidateTaskId?: string | null
  ): T | null {
    const trackedId = instanceUpdateTaskIds.value[instanceName] || candidateTaskId;
    if (trackedId) {
      const task = tasks.find((t) => t.id === trackedId);
      if (
        task &&
        isModpackUpdateTask(task.task_type) &&
        getModpackTaskInstanceName(task.task_type) === instanceName &&
        !isTaskDismissed(task.id) &&
        (isTaskActive(task.status) || isTaskTerminal(task.status))
      ) {
        if (instanceUpdateTaskIds.value[instanceName] !== task.id) {
          setInstanceUpdateTaskId(instanceName, task.id);
        }
        return task;
      }
    }

    const untracked = findActiveUntrackedInstanceUpdateTask(tasks, instanceName);
    if (untracked) {
      setInstanceUpdateTaskId(instanceName, untracked.id);
      return untracked;
    }

    return null;
  }

  function hasActiveDownloadsModpackTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[]
  ): boolean {
    return Boolean(findRestorableDownloadsModpackTask(tasks));
  }

  function hasOngoingInstanceUpdateTask<T extends { id: string; status: any; task_type: any }>(
    tasks: T[],
    instanceName: string
  ): boolean {
    return Boolean(findRestorableInstanceUpdateTask(tasks, instanceName));
  }

  return {
    downloadsModpackTaskId,
    instanceUpdateTaskIds,
    setDownloadsModpackTaskId,
    setInstanceUpdateTaskId,
    dismissTask,
    isTaskDismissed,
    clearState,
    findActiveUntrackedDownloadsModpackTask,
    findActiveUntrackedInstanceUpdateTask,
    findRestorableDownloadsModpackTask,
    findRestorableInstanceUpdateTask,
    hasActiveDownloadsModpackTask,
    hasOngoingInstanceUpdateTask,
  };
}
