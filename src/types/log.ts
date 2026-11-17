export type LogLevel = 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE';
export type LogLevelSetting = 'error' | 'warn' | 'info' | 'debug' | 'trace';
export type LogSource = 'rust' | 'frontend';

export interface LogEntry {
  seq?: number;
  message: string;
  level: string;
  target: string;
  source: LogSource | string;
  task_id: string | null;
  instance_id?: string | null;
  timestamp: string;
}

export interface FrontendLogEntry {
  level: LogLevel;
  target: string;
  message: string;
  taskId: string | null;
}

export interface LogFileInfo {
  name: string;
  sizeBytes: number;
  modifiedAt: number;
}

export interface EffectiveLogConfig {
  configuredLevel: LogLevelSetting | null;
  effectiveLevel: LogLevelSetting;
  retentionDays: number;
  maxTotalMb: number;
  logDir: string;
}
