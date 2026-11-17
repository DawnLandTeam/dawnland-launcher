import { invoke } from '@tauri-apps/api/core';
import type { FrontendLogEntry, LogEntry, LogLevel, LogLevelSetting } from '../types/log';

const LEVEL_RANK: Record<LogLevel, number> = {
  ERROR: 0,
  WARN: 1,
  INFO: 2,
  DEBUG: 3,
  TRACE: 4,
};

const MAX_PENDING_QUEUE = 200;
const BATCH_FLUSH_SIZE = 20;
const BATCH_FLUSH_MS = 100;

let effectiveLevel: LogLevel = import.meta.env.DEV ? 'DEBUG' : 'INFO';
let uiSink: ((entry: LogEntry) => void) | null = null;
let customTransport: ((entries: FrontendLogEntry[]) => Promise<void>) | null = null;

const pendingQueue: FrontendLogEntry[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function formatTimestampNow(): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
}

function serializeArg(arg: unknown): string {
  if (arg instanceof Error) {
    return arg.message ? `${arg.name}: ${arg.message}` : arg.name;
  }
  if (typeof arg === 'string') {
    return arg;
  }
  if (typeof arg === 'number' || typeof arg === 'boolean' || arg === null || arg === undefined) {
    return String(arg);
  }
  if (typeof arg === 'object') {
    const rec = arg as { code?: unknown; message?: unknown; data?: unknown };
    if (typeof rec.code === 'string' && typeof rec.message === 'string') {
      return `[${rec.code}] ${rec.message}`;
    }
    try {
      return JSON.stringify(arg);
    } catch {
      return Object.prototype.toString.call(arg);
    }
  }
  return String(arg);
}

function formatMessage(args: unknown[]): string {
  return args.map(serializeArg).filter((s) => s.length > 0).join(' ');
}

function shouldEmitToUi(level: LogLevel): boolean {
  return LEVEL_RANK[level] <= LEVEL_RANK[effectiveLevel];
}

function shouldPersistToDisk(level: LogLevel): boolean {
  const rank = LEVEL_RANK[level];
  const effectiveRank = LEVEL_RANK[effectiveLevel];
  // In production default INFO mode, only persist frontend WARN/ERROR to disk.
  // When user switches to DEBUG or TRACE, persist INFO/DEBUG (and TRACE) as well.
  if (!import.meta.env.DEV && effectiveRank <= LEVEL_RANK.INFO) {
    return rank <= LEVEL_RANK.WARN;
  }
  return rank <= effectiveRank;
}

function writeToConsole(level: LogLevel, scope: string, args: unknown[]): void {
  const prefix = `[${scope}]`;
  const effectiveRank = LEVEL_RANK[effectiveLevel];
  const rank = LEVEL_RANK[level];

  if (!import.meta.env.DEV && effectiveRank <= LEVEL_RANK.INFO && rank > LEVEL_RANK.WARN) {
    return;
  }
  if (rank > effectiveRank) {
    return;
  }

  switch (level) {
    case 'ERROR':
      console.error(prefix, ...args);
      break;
    case 'WARN':
      console.warn(prefix, ...args);
      break;
    case 'INFO':
      console.info(prefix, ...args);
      break;
    case 'DEBUG':
    case 'TRACE':
      console.debug(prefix, ...args);
      break;
  }
}

async function flushQueue(): Promise<void> {
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  if (pendingQueue.length === 0) {
    return;
  }

  const batch = pendingQueue.splice(0, pendingQueue.length);
  const transport = customTransport;

  if (transport) {
    await Promise.resolve(transport(batch)).catch(() => {
      // Never re-enter logger inside transport error handling
    });
    return;
  }

  if (!isTauriRuntime()) {
    return;
  }

  await Promise.resolve(invoke('log_frontend_batch', { entries: batch })).catch(() => {
    // Avoid infinite recursion if IPC transport fails during shutdown
  });
}

function scheduleFlush(): void {
  if (pendingQueue.length >= BATCH_FLUSH_SIZE) {
    void flushQueue();
    return;
  }
  if (flushTimer === null) {
    flushTimer = setTimeout(() => {
      flushTimer = null;
      void flushQueue();
    }, BATCH_FLUSH_MS);
  }
}

function emitLog(level: LogLevel, scope: string, taskId: string | null, args: unknown[]): void {
  if (args.length === 0) return;

  writeToConsole(level, scope, args);

  const message = formatMessage(args);
  if (!message) return;

  if (uiSink && shouldEmitToUi(level)) {
    uiSink({
      message,
      level,
      target: scope,
      source: 'frontend',
      task_id: taskId,
      timestamp: formatTimestampNow(),
    });
  }

  if (shouldPersistToDisk(level) && (customTransport !== null || isTauriRuntime())) {
    if (pendingQueue.length >= MAX_PENDING_QUEUE) {
      pendingQueue.shift();
    }
    pendingQueue.push({
      level,
      target: scope,
      message,
      taskId,
    });
    scheduleFlush();
  }
}

export function registerLogUiSink(sink: ((entry: LogEntry) => void) | null): void {
  uiSink = sink;
}

export function setFrontendEffectiveLogLevel(level: LogLevelSetting | LogLevel): void {
  const upper = level.toUpperCase() as LogLevel;
  if (upper in LEVEL_RANK) {
    effectiveLevel = upper;
  }
}

export function getFrontendEffectiveLogLevel(): LogLevel {
  return effectiveLevel;
}

export interface ScopedLogger {
  debug: (...args: unknown[]) => void;
  info: (...args: unknown[]) => void;
  warn: (...args: unknown[]) => void;
  error: (...args: unknown[]) => void;
  withTask: (taskId: string) => Omit<ScopedLogger, 'withTask'>;
}

export function createLogger(scope: string): ScopedLogger {
  return {
    debug: (...args: unknown[]) => emitLog('DEBUG', scope, null, args),
    info: (...args: unknown[]) => emitLog('INFO', scope, null, args),
    warn: (...args: unknown[]) => emitLog('WARN', scope, null, args),
    error: (...args: unknown[]) => emitLog('ERROR', scope, null, args),
    withTask: (taskId: string) => ({
      debug: (...args: unknown[]) => emitLog('DEBUG', scope, taskId, args),
      info: (...args: unknown[]) => emitLog('INFO', scope, taskId, args),
      warn: (...args: unknown[]) => emitLog('WARN', scope, taskId, args),
      error: (...args: unknown[]) => emitLog('ERROR', scope, taskId, args),
    }),
  };
}

export function __setLoggerTransportForTest(
  transport: ((entries: FrontendLogEntry[]) => Promise<void>) | null,
): void {
  customTransport = transport;
}

export async function __flushLoggerForTest(): Promise<void> {
  await flushQueue();
}

export function __resetLoggerForTest(): void {
  pendingQueue.splice(0, pendingQueue.length);
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  customTransport = null;
  effectiveLevel = import.meta.env.DEV ? 'DEBUG' : 'INFO';
}
