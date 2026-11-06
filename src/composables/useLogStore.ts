import { ref } from 'vue';
import { listen } from '@tauri-apps/api/event';

export interface LogEntry {
  message: string;
  level: string;
  task_id: string | null;
  timestamp: string;
}

const globalLogs = ref<LogEntry[]>([]);
const taskLogs = ref<Record<string, LogEntry[]>>({});
const taskOrder: string[] = [];
let isInitialized = false;

export function useLogStore() {
  function appendLog(log: LogEntry) {
    globalLogs.value.push(log);

    if (log.task_id) {
      if (!taskLogs.value[log.task_id]) {
        taskLogs.value[log.task_id] = [];
        taskOrder.push(log.task_id);
        // Keep only the logs for the last 20 tasks
        if (taskOrder.length > 20) {
          const oldTaskId = taskOrder.shift();
          if (oldTaskId) {
            delete taskLogs.value[oldTaskId];
          }
        }
      }
      taskLogs.value[log.task_id].push(log);
      
      // Limit logs per task to 2000 lines efficiently
      if (taskLogs.value[log.task_id].length > 2000) {
        taskLogs.value[log.task_id].splice(0, taskLogs.value[log.task_id].length - 2000);
      }
    }
  }

  function trimGlobalLogs() {
    if (globalLogs.value.length > 1000) {
      globalLogs.value.splice(0, globalLogs.value.length - 1000);
    }
  }

  async function init() {
    if (isInitialized) return;
    isInitialized = true;
    
    await listen<LogEntry>('app-log', (event) => {
      appendLog(event.payload);
      trimGlobalLogs();
    });

    await listen<LogEntry[]>('app-logs-batch', (event) => {
      const batch = event.payload;
      for (const log of batch) {
        appendLog(log);
      }
      trimGlobalLogs();
    });
  }

  function getTaskErrorLogs(taskId: string): string {
    const logs = taskLogs.value[taskId];
    if (!logs) return '';
    
    return logs
      .filter(log => log.level.toUpperCase() === 'ERROR' || log.level.toUpperCase() === 'WARN' || log.message.toLowerCase().includes('failed') || log.message.toLowerCase().includes('error'))
      .map(log => `[${log.timestamp}] [${log.level.toUpperCase()}] ${log.message}`)
      .join('\n');
  }

  return {
    globalLogs,
    taskLogs,
    init,
    getTaskErrorLogs,
  };
}
