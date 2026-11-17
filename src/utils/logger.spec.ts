import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  createLogger,
  registerLogUiSink,
  setFrontendEffectiveLogLevel,
  getFrontendEffectiveLogLevel,
  __setLoggerTransportForTest,
  __flushLoggerForTest,
  __resetLoggerForTest,
} from './logger';
import type { FrontendLogEntry, LogEntry } from '../types/log';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

describe('unified frontend logger', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    __resetLoggerForTest();
    registerLogUiSink(null);
  });

  afterEach(() => {
    registerLogUiSink(null);
    __resetLoggerForTest();
    vi.useRealTimers();
    const win = window as unknown as Record<string, unknown>;
    delete win.__TAURI_INTERNALS__;
  });

  it('filters UI sink emissions according to effective log level', () => {
    const received: LogEntry[] = [];
    registerLogUiSink((entry) => received.push(entry));
    const consoleDebugSpy = vi.spyOn(console, 'debug').mockImplementation(() => {});
    const consoleInfoSpy = vi.spyOn(console, 'info').mockImplementation(() => {});
    const consoleWarnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    setFrontendEffectiveLogLevel('warn');
    expect(getFrontendEffectiveLogLevel()).toBe('WARN');

    const log = createLogger('TestScope');
    log.debug('hidden debug');
    log.info('hidden info');
    log.warn('visible warn');

    expect(received).toHaveLength(1);
    expect(received[0].level).toBe('WARN');
    expect(received[0].target).toBe('TestScope');
    expect(received[0].source).toBe('frontend');
    expect(received[0].message).toBe('visible warn');

    setFrontendEffectiveLogLevel('debug');
    log.debug('visible debug');
    expect(received).toHaveLength(2);
    expect(received[1].level).toBe('DEBUG');

    consoleDebugSpy.mockRestore();
    consoleInfoSpy.mockRestore();
    consoleWarnSpy.mockRestore();
  });

  it('attaches taskId when using withTask()', async () => {
    const received: LogEntry[] = [];
    const transported: FrontendLogEntry[] = [];
    registerLogUiSink((entry) => received.push(entry));
    __setLoggerTransportForTest(async (entries) => {
      transported.push(...entries);
    });
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const log = createLogger('DownloadTab');
    log.withTask('task-42').error('Download aborted');

    expect(received).toHaveLength(1);
    expect(received[0].task_id).toBe('task-42');

    await __flushLoggerForTest();
    expect(transported).toEqual([
      {
        level: 'ERROR',
        target: 'DownloadTab',
        message: 'Download aborted',
        taskId: 'task-42',
      },
    ]);

    consoleErrorSpy.mockRestore();
  });

  it('micro-batches pending entries and auto-flushes on timer and batch threshold', async () => {
    vi.useFakeTimers();
    const batches: FrontendLogEntry[][] = [];
    __setLoggerTransportForTest(async (entries) => {
      batches.push(entries);
    });
    const consoleWarnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

    const log = createLogger('BatchTest');
    log.warn('first');
    log.warn('second');
    expect(batches).toHaveLength(0);

    await vi.advanceTimersByTimeAsync(120);
    expect(batches).toHaveLength(1);
    expect(batches[0].map((e) => e.message)).toEqual(['first', 'second']);

    for (let i = 0; i < 20; i++) {
      log.warn(`item-${i}`);
    }
    await Promise.resolve();
    expect(batches).toHaveLength(2);
    expect(batches[1]).toHaveLength(20);

    consoleWarnSpy.mockRestore();
  });

  it('formats Error and structured AppError objects cleanly', () => {
    const received: LogEntry[] = [];
    registerLogUiSink((entry) => received.push(entry));
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const log = createLogger('ErrorFormat');
    log.error('Native error:', new Error('disk full'));
    log.error('IPC error:', { code: 'MD5_MISMATCH', message: 'Checksum failed' });
    log.error('Meta:', { retries: 3 });

    expect(received[0].message).toBe('Native error: Error: disk full');
    expect(received[1].message).toBe('IPC error: [MD5_MISMATCH] Checksum failed');
    expect(received[2].message).toBe('Meta: {"retries":3}');

    consoleErrorSpy.mockRestore();
  });

  it('does not recurse when transport fails and gates Tauri IPC behind runtime check', async () => {
    const received: LogEntry[] = [];
    registerLogUiSink((entry) => received.push(entry));
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const log = createLogger('Safety');
    log.error('non-tauri error');
    await __flushLoggerForTest();
    expect(invoke).not.toHaveBeenCalled();

    const win = window as unknown as Record<string, unknown>;
    win.__TAURI_INTERNALS__ = {};
    vi.mocked(invoke).mockRejectedValueOnce(new Error('IPC channel closed'));

    log.error('tauri error');
    await __flushLoggerForTest();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('log_frontend_batch', {
      entries: [
        {
          level: 'ERROR',
          target: 'Safety',
          message: 'tauri error',
          taskId: null,
        },
      ],
    });
    expect(received).toHaveLength(2);

    consoleErrorSpy.mockRestore();
  });
});
