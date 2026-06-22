export type TaskType = 
  | { InstallVanilla: { version_id: string; version_json_url: string; is_dependency?: boolean } }
  | { InstallForge: { mc_version: string; loader_version: string; loader_type: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallFabric: { mc_version: string; fabric_version: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallModpack: { zip_path: string; instance_name: string; is_update: boolean; project_id?: string } }
  | { InstallOnlineModpack: { url: string; instance_name: string; is_update: boolean; project_id?: string } }
  | { InstallMod: { source: string; project_id: string; mod_name: string; instance_id: string; target_dir: string; download_url: string; file_id: string; keep_both: boolean } }
  | { InstallResourcepack: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallShaderpack: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallWorld: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
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
  speed: number;
  remaining_files: number;
  sub_tasks: SubTaskState[];
}

export interface SubTaskState {
  key: string;
  name: string;
  status: TaskStatus;
  current: number;
  total: number;
  weight: number;
}

export interface TaskState {
  id: string;
  task_type: TaskType;
  status: TaskStatus;
  progress: TaskProgress;
  error: string | null;
  created_at: number;
  updated_at: number;
  auto_clear: boolean;
}

import i18n from '../i18n';

export function getTaskName(taskType: TaskType): string {
  const t = i18n.global.t;
  if ('InstallVanilla' in taskType) return t('task.installVanilla', { version: taskType.InstallVanilla.version_id });
  if ('InstallForge' in taskType) return t('task.installLoader', { loader: taskType.InstallForge.loader_type, version: taskType.InstallForge.mc_version });
  if ('InstallFabric' in taskType) return t('task.installLoader', { loader: 'Fabric', version: taskType.InstallFabric.mc_version });
  if ('InstallModpack' in taskType) return t('task.installModpack', { name: taskType.InstallModpack.instance_name });
  if ('InstallOnlineModpack' in taskType) return t('task.downloadModpack', { name: taskType.InstallOnlineModpack.instance_name });
  if ('InstallMod' in taskType) return t('task.installMod', { name: taskType.InstallMod.mod_name });
  if ('InstallResourcepack' in taskType) return t('task.installResourcepack', { name: taskType.InstallResourcepack.pack_name });
  if ('InstallShaderpack' in taskType) return t('task.installShaderpack', { name: taskType.InstallShaderpack.pack_name });
  if ('InstallWorld' in taskType) return t('task.installWorld', { name: taskType.InstallWorld.pack_name });
  if ('Generic' in taskType) return taskType.Generic.name;
  return t('task.unknown');
}
