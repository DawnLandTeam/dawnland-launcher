import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useLogStore } from './useLogStore';
import { getFrontendEffectiveLogLevel, __resetLoggerForTest } from '../utils/logger';
import type { LogEntry } from '../types/log';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

describe('useLogStore composable', () => {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();

  beforeEach(() => {
    vi.clearAllMocks();
    listeners.clear();
    __resetLoggerForTest();
    const store = useLogStore();
    store.__resetForTest();

    vi.mocked(listen).mockImplementation(async (eventName, handler) => {
      listeners.set(eventName, handler as (event: { payload: unknown }) => void);
      return () => {
        listeners.delete(eventName);
      };
    });
  });

  it('catches up recent logs on init and deduplicates live events by seq', async () => {
    const startupLogs: LogEntry[] = [
      {
        seq: 1,
        timestamp: '2026-11-16 10:00:00',
        level: 'INFO',
        target: 'dawnland_launcher_lib::core',
        source: 'rust',
        task_id: null,
        message: 'Launcher initialized',
      },
      {
        seq: 2,
        timestamp: '2026-11-16 10:00:01',
        level: 'WARN',
        target: 'dawnland_launcher_lib::auth',
        source: 'rust',
        task_id: 'task-1',
        message: 'Retrying token refresh',
      },
    ];

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_log_config') {
        return {
          configuredLevel: 'debug',
          effectiveLevel: 'debug',
          retentionDays: 7,
          maxTotalMb: 50,
          logDir: 'C:/dawnland/logs',
        };
      }
      if (cmd === 'get_recent_logs') {
        return startupLogs;
      }
      return null;
    });

    const store = useLogStore();
    await store.init();

    expect(store.logConfig.value.effectiveLevel).toBe('debug');
    expect(getFrontendEffectiveLogLevel()).toBe('DEBUG');
    expect(store.globalLogs.value).toHaveLength(2);

    const singleHandler = listeners.get('app-log');
    const batchHandler = listeners.get('app-logs-batch');
    expect(singleHandler).toBeDefined();
    expect(batchHandler).toBeDefined();

    // Duplicate seq 2 should be ignored
    singleHandler?.({
      payload: {
        seq: 2,
        timestamp: '2026-11-16 10:00:01',
        level: 'WARN',
        target: 'dawnland_launcher_lib::auth',
        source: 'rust',
        task_id: 'task-1',
        message: 'Retrying token refresh',
      },
    });

    batchHandler?.({
      payload: [
        {
          seq: 2,
          timestamp: '2026-11-16 10:00:01',
          level: 'WARN',
          target: 'dawnland_launcher_lib::auth',
          source: 'rust',
          task_id: 'task-1',
          message: 'Retrying token refresh',
        },
        {
          seq: 3,
          timestamp: '2026-11-16 10:00:02',
          level: 'ERROR',
          target: 'dawnland_launcher_lib::downloader',
          source: 'rust',
          task_id: 'task-1',
          message: 'Checksum verification failed',
        },
      ],
    });

    expect(store.globalLogs.value).toHaveLength(3);
    expect(store.taskLogs.value['task-1']).toHaveLength(2);
    expect(store.getTaskErrorLogs('task-1')).toContain('[WARN] [dawnland_launcher_lib::auth] Retrying token refresh');
    expect(store.getTaskErrorLogs('task-1')).toContain('[ERROR] [dawnland_launcher_lib::downloader] Checksum verification failed');
  });

  it('clearDisplayLogs clears globalLogs without erasing taskLogs', () => {
    const store = useLogStore();
    store.__appendForTest({
      seq: 10,
      timestamp: '2026-11-16 10:05:00',
      level: 'ERROR',
      target: 'InstallModpack',
      source: 'rust',
      task_id: 'task-keep',
      message: 'Failed to unpack mod archive',
    });

    expect(store.globalLogs.value).toHaveLength(1);
    expect(store.taskLogs.value['task-keep']).toHaveLength(1);

    store.clearDisplayLogs();
    expect(store.globalLogs.value).toHaveLength(0);
    expect(store.taskLogs.value['task-keep']).toHaveLength(1);
  });

  it('updates log level and forwards log file management commands', async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      configuredLevel: 'trace',
      effectiveLevel: 'trace',
      retentionDays: 14,
      maxTotalMb: 50,
      logDir: 'C:/dawnland/logs',
    });

    const store = useLogStore();
    const updated = await store.setLogLevel('trace', 14);

    expect(invoke).toHaveBeenCalledWith('set_log_level', {
      level: 'trace',
      retentionDays: 14,
    });
    expect(updated.effectiveLevel).toBe('trace');
    expect(getFrontendEffectiveLogLevel()).toBe('TRACE');

    await store.listLogFiles();
    expect(invoke).toHaveBeenCalledWith('list_log_files');

    await store.readLogFile('dawnland.log.2026-11-16', 250);
    expect(invoke).toHaveBeenCalledWith('read_log_file', {
      filename: 'dawnland.log.2026-11-16',
      tailLines: 250,
    });

    await store.openLogDirectory();
    expect(invoke).toHaveBeenCalledWith('open_log_directory');

    await store.exportDiagnosticLogs('C:/tmp/diag.zip', true, '1.20.1');
    expect(invoke).toHaveBeenCalledWith('export_diagnostic_logs', {
      destPath: 'C:/tmp/diag.zip',
      includeGameLogs: true,
      instanceId: '1.20.1',
    });
  });
});
