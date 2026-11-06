export type TaskType = 
  | { InstallVanilla: { version_id: string; version_json_url: string; custom_instance_name?: string; is_dependency?: boolean } }
  | { InstallForge: { mc_version: string; loader_version: string; loader_type: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallFabric: { mc_version: string; fabric_version: string; custom_instance_name: string; is_dependency?: boolean } }
  | { InstallModpack: { zip_path: string; instance_name: string; is_update: boolean; project_id?: string | null; server_id?: string | null; pack_version_id?: string | null; pack_file_name?: string | null } }
  | { InstallOnlineModpack: { url: string; instance_name: string; is_update: boolean; project_id?: string | null; server_id?: string | null; pack_version_id?: string | null; pack_file_name?: string | null } }
  | { InstallMod: { source: string; project_id: string; mod_name: string; instance_id: string; target_dir: string; download_url: string; file_id: string; keep_both: boolean } }
  | { InstallResourcepack: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallShaderpack: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallWorld: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallDatapack: { source: string; project_id: string; pack_name: string; instance_id?: string | null; target_dir?: string | null; download_url: string; file_id: string; } }
  | { InstallPreset: { preset_name: string; asset_type?: string; instance_id: string; } }
  | { Generic: { name: string } };

export type TaskStatus = 
  | 'Pending'
  | 'Running'
  | 'Paused'
  | 'Cancelling'
  | 'Completed'
  | 'Failed'
  | 'Cancelled';

export function isTaskActive(status: TaskStatus): boolean {
  return status === 'Pending' || status === 'Running' || status === 'Paused' || status === 'Cancelling';
}

export function isTaskCancelling(status: TaskStatus): boolean {
  return status === 'Cancelling';
}

export function isTaskTerminal(status: TaskStatus): boolean {
  return status === 'Completed' || status === 'Failed' || status === 'Cancelled';
}

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
  if ('InstallVanilla' in taskType) return t('task.installVanilla', { version: taskType.InstallVanilla.custom_instance_name || taskType.InstallVanilla.version_id });
  if ('InstallForge' in taskType) return t('task.installLoader', { loader: taskType.InstallForge.loader_type, version: taskType.InstallForge.mc_version });
  if ('InstallFabric' in taskType) return t('task.installLoader', { loader: 'Fabric', version: taskType.InstallFabric.mc_version });
  if ('InstallModpack' in taskType) return t('task.installModpack', { name: taskType.InstallModpack.instance_name });
  if ('InstallOnlineModpack' in taskType) return t('task.downloadModpack', { name: taskType.InstallOnlineModpack.instance_name });
  if ('InstallMod' in taskType) return t('task.installMod', { name: taskType.InstallMod.mod_name });
  if ('InstallResourcepack' in taskType) return t('task.installResourcepack', { name: taskType.InstallResourcepack.pack_name });
  if ('InstallShaderpack' in taskType) return t('task.installShaderpack', { name: taskType.InstallShaderpack.pack_name });
  if ('InstallWorld' in taskType) return t('task.installWorld', { name: taskType.InstallWorld.pack_name });
  if ('InstallDatapack' in taskType) return t('task.installDatapack', { name: taskType.InstallDatapack.pack_name });
  if ('InstallPreset' in taskType) return t('task.installPreset', { name: taskType.InstallPreset.preset_name });
  if ('Generic' in taskType) return taskType.Generic.name;
  return t('task.unknown');
}

export interface TaskTargetRoute {
  path: string;
  query?: Record<string, string>;
}

export function isModpackTask(taskType: TaskType): boolean {
  return 'InstallModpack' in taskType || 'InstallOnlineModpack' in taskType;
}

export function isModpackUpdateTask(taskType: TaskType): boolean {
  if ('InstallModpack' in taskType) {
    return taskType.InstallModpack.is_update;
  }
  if ('InstallOnlineModpack' in taskType) {
    return taskType.InstallOnlineModpack.is_update;
  }
  return false;
}

export function getModpackTaskInstanceName(taskType: TaskType): string | null {
  if ('InstallModpack' in taskType) {
    return taskType.InstallModpack.instance_name;
  }
  if ('InstallOnlineModpack' in taskType) {
    return taskType.InstallOnlineModpack.instance_name;
  }
  return null;
}

export function getTaskTargetRoute(taskType: TaskType): TaskTargetRoute | null {
  if ('InstallModpack' in taskType) {
    if (taskType.InstallModpack.is_update) {
      return {
        path: `/instances/${encodeURIComponent(taskType.InstallModpack.instance_name)}`,
        query: { tab: 'update_modpack' }
      };
    }
    return {
      path: '/downloads',
      query: { tab: 'modpack' }
    };
  }
  if ('InstallOnlineModpack' in taskType) {
    if (taskType.InstallOnlineModpack.is_update) {
      return {
        path: `/instances/${encodeURIComponent(taskType.InstallOnlineModpack.instance_name)}`,
        query: { tab: 'update_modpack' }
      };
    }
    return {
      path: '/downloads',
      query: { tab: 'modpack' }
    };
  }
  if ('InstallVanilla' in taskType) {
    return {
      path: '/downloads',
      query: { tab: 'instance', install_version: taskType.InstallVanilla.version_id }
    };
  }
  if ('InstallForge' in taskType) {
    return {
      path: '/downloads',
      query: { tab: 'instance', install_version: taskType.InstallForge.mc_version, install_loader: 'forge' }
    };
  }
  if ('InstallFabric' in taskType) {
    return {
      path: '/downloads',
      query: { tab: 'instance', install_version: taskType.InstallFabric.mc_version, install_loader: 'fabric' }
    };
  }
  if ('InstallMod' in taskType) {
    return {
      path: `/instances/${encodeURIComponent(taskType.InstallMod.instance_id)}`,
      query: { tab: 'mods' }
    };
  }
  if ('InstallResourcepack' in taskType && taskType.InstallResourcepack.instance_id) {
    return {
      path: `/instances/${encodeURIComponent(taskType.InstallResourcepack.instance_id)}`,
      query: { tab: 'resourcepacks' }
    };
  }
  if ('InstallShaderpack' in taskType && taskType.InstallShaderpack.instance_id) {
    return {
      path: `/instances/${encodeURIComponent(taskType.InstallShaderpack.instance_id)}`,
      query: { tab: 'shaders' }
    };
  }
  if ('InstallDatapack' in taskType && taskType.InstallDatapack.instance_id) {
    return {
      path: `/instances/${encodeURIComponent(taskType.InstallDatapack.instance_id)}`,
      query: { tab: 'datapacks' }
    };
  }
  if ('InstallWorld' in taskType && taskType.InstallWorld.instance_id) {
    return {
      path: `/instances/${encodeURIComponent(taskType.InstallWorld.instance_id)}`,
      query: { tab: 'worlds' }
    };
  }
  return null;
}

export function formatTaskErrorMessage(
  error: string | null | undefined,
  t: (key: string, defaultMsg?: string) => string
): string {
  if (!error) return "";
  if (error === 'Task cancelled' || error.includes('cancelled by user')) {
    return t('task.cancelledError', 'Task was cancelled');
  }
  if (error.includes('GAME_IS_RUNNING')) {
    return t('modpack.instanceRunningWarning', 'The instance is currently running. Please close the game first.');
  }
  return error;
}
