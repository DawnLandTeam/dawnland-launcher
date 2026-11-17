import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { EffectiveLogConfig, LogEntry, LogFileInfo, LogLevelSetting } from '../types/log';
import {
  registerLogUiSink,
  setFrontendEffectiveLogLevel,
} from '../utils/logger';

const MAX_GLOBAL_LOGS = 1000;
const MAX_TASK_LOGS = 20;
const MAX_LINES_PER_TASK = 2000;

const globalLogs = ref<LogEntry[]>([]);
const taskLogs = ref<Record<string, LogEntry[]>>({});
const logConfig = ref<EffectiveLogConfig>({
  configuredLevel: null,
  effectiveLevel: import.meta.env.DEV ? 'debug' : 'info',
  retentionDays: 7,
  maxTotalMb: 50,
  logDir: '',
});

const taskOrder: string[] = [];
const seenSeqs = new Set<number>();
let isInitialized = false;

function normalizeLogEntry(raw: LogEntry): LogEntry {
  return {
    ...raw,
    target: raw.target || 'app',
    source: raw.source || 'rust',
  };
}

function appendLog(rawLog: LogEntry): void {
  const log = normalizeLogEntry(rawLog);
  if (typeof log.seq === 'number') {
    if (seenSeqs.has(log.seq)) {
      return;
    }
    seenSeqs.add(log.seq);
    if (seenSeqs.size > 4000) {
      const oldest = seenSeqs.values().next().value;
      if (typeof oldest === 'number') {
        seenSeqs.delete(oldest);
      }
    }
  }

  globalLogs.value.push(log);

  if (log.task_id) {
    if (!taskLogs.value[log.task_id]) {
      taskLogs.value[log.task_id] = [];
      taskOrder.push(log.task_id);
      // Keep only the logs for the last 20 tasks
      if (taskOrder.length > MAX_TASK_LOGS) {
        const oldTaskId = taskOrder.shift();
        if (oldTaskId) {
          delete taskLogs.value[oldTaskId];
        }
      }
    }
    taskLogs.value[log.task_id].push(log);

    // Limit logs per task efficiently
    if (taskLogs.value[log.task_id].length > MAX_LINES_PER_TASK) {
      taskLogs.value[log.task_id].splice(
        0,
        taskLogs.value[log.task_id].length - MAX_LINES_PER_TASK,
      );
    }
  }
}

function trimGlobalLogs(): void {
  if (globalLogs.value.length > MAX_GLOBAL_LOGS) {
    globalLogs.value.splice(0, globalLogs.value.length - MAX_GLOBAL_LOGS);
  }
}

// Register frontend logger UI sink immediately so early Vue mount logs appear in Log Center
registerLogUiSink((entry) => {
  appendLog(entry);
  trimGlobalLogs();
});

export function useLogStore() {
  async function loadLogConfig(): Promise<EffectiveLogConfig> {
    const config = await invoke<EffectiveLogConfig>('get_log_config');
    if (config) {
      logConfig.value = config;
      setFrontendEffectiveLogLevel(config.effectiveLevel);
    }
    return logConfig.value;
  }

  async function setLogLevel(
    level: LogLevelSetting | null,
    retentionDays?: number,
  ): Promise<EffectiveLogConfig> {
    const updated = await invoke<EffectiveLogConfig>('set_log_level', {
      level,
      retentionDays: retentionDays ?? null,
    });
    if (updated) {
      logConfig.value = updated;
      setFrontendEffectiveLogLevel(updated.effectiveLevel);
    }
    return logConfig.value;
  }

  async function init(): Promise<void> {
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

    try {
      await loadLogConfig();
    } catch {
      // Ignore during non-Tauri or test contexts
    }

    try {
      const recent = await invoke<LogEntry[]>('get_recent_logs');
      if (Array.isArray(recent)) {
        for (const entry of recent) {
          appendLog(entry);
        }
        trimGlobalLogs();
      }
    } catch {
      // Ignore during non-Tauri or test contexts
    }
  }

  function clearDisplayLogs(): void {
    globalLogs.value = [];
  }

  function clearLogs(): void {
    clearDisplayLogs();
  }

  function getTaskErrorLogs(taskId: string): string {
    const logs = taskLogs.value[taskId];
    if (!logs) return '';

    return logs
      .filter(
        (log) =>
          log.level.toUpperCase() === 'ERROR' ||
          log.level.toUpperCase() === 'WARN' ||
          log.message.toLowerCase().includes('failed') ||
          log.message.toLowerCase().includes('error'),
      )
      .map(
        (log) =>
          `[${log.timestamp}] [${log.level.toUpperCase()}] [${log.target}] ${log.message}`,
      )
      .join('\n');
  }

  async function listLogFiles(): Promise<LogFileInfo[]> {
    return invoke<LogFileInfo[]>('list_log_files');
  }

  async function readLogFile(filename: string, tailLines = 500): Promise<string[]> {
    return invoke<string[]>('read_log_file', { filename, tailLines });
  }

  async function openLogDirectory(): Promise<void> {
    await invoke('open_log_directory');
  }

  async function exportDiagnosticLogs(
    destPath: string,
    includeGameLogs = false,
    instanceId?: string,
  ): Promise<string> {
    return invoke<string>('export_diagnostic_logs', {
      destPath,
      includeGameLogs,
      instanceId: instanceId ?? null,
    });
  }

  function __resetForTest(): void {
    globalLogs.value = [];
    taskLogs.value = {};
    taskOrder.splice(0, taskOrder.length);
    seenSeqs.clear();
    isInitialized = false;
  }

  return {
    globalLogs,
    taskLogs,
    logConfig,
    init,
    loadLogConfig,
    setLogLevel,
    clearDisplayLogs,
    clearLogs,
    getTaskErrorLogs,
    listLogFiles,
    readLogFile,
    openLogDirectory,
    exportDiagnosticLogs,
    __appendForTest: (entry: LogEntry) => {
      appendLog(entry);
      trimGlobalLogs();
    },
    __resetForTest,
  };
}
