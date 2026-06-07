export type TaskType = 
  | { InstallVanilla: { version_id: string; version_json_url: string; is_dependency?: boolean } }
  | { InstallForge: { mc_version: string; loader_version: string; loader_type: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallFabric: { mc_version: string; fabric_version: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallModpack: { zip_path: string; instance_name: string; is_update: boolean; project_id?: string } }
  | { InstallOnlineModpack: { url: string; instance_name: string; is_update: boolean; project_id?: string } }
  | { Generic: { name: string } };

export type TaskStatus = 
  | 'Pending'
  | 'Running'
  | 'Paused'
  | 'Completed'
  | 'Failed'
  | 'Cancelled';

export interface TaskProgress {
  current: number;
  total: number;
  step: number;
  total_steps: number;
  detail: string;
}

export interface TaskState {
  id: string;
  task_type: TaskType;
  status: TaskStatus;
  progress: TaskProgress;
  error: string | null;
  created_at: number;
  updated_at: number;
}

import i18n from '../i18n';

export function getTaskName(taskType: TaskType): string {
  const t = i18n.global.t;
  if ('InstallVanilla' in taskType) return t('task.installVanilla', { version: taskType.InstallVanilla.version_id });
  if ('InstallForge' in taskType) return t('task.installLoader', { loader: taskType.InstallForge.loader_type, version: taskType.InstallForge.mc_version });
  if ('InstallFabric' in taskType) return t('task.installLoader', { loader: 'Fabric', version: taskType.InstallFabric.mc_version });
  if ('InstallModpack' in taskType) return t('task.installModpack', { name: taskType.InstallModpack.instance_name });
  if ('InstallOnlineModpack' in taskType) return t('task.downloadModpack', { name: taskType.InstallOnlineModpack.instance_name });
  if ('Generic' in taskType) return taskType.Generic.name;
  return t('task.unknown');
}
