import { ref } from 'vue';

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

  return {
    downloadsModpackTaskId,
    instanceUpdateTaskIds,
    setDownloadsModpackTaskId,
    setInstanceUpdateTaskId,
    dismissTask,
    isTaskDismissed,
    clearState,
  };
}
