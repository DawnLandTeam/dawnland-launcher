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
let isInitialized = false;

export function useLogStore() {
  async function init() {
    if (isInitialized) return;
    isInitialized = true;
    
    await listen<LogEntry>('app-log', (event) => {
      const log = event.payload;
      globalLogs.value.push(log);
      // Keep only last 1000 logs to prevent memory leak
      if (globalLogs.value.length > 1000) {
        globalLogs.value.shift();
      }

      if (log.task_id) {
        if (!taskLogs.value[log.task_id]) {
          taskLogs.value[log.task_id] = [];
        }
        taskLogs.value[log.task_id].push(log);
      }
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
