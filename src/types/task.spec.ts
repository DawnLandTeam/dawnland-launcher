import { describe, it, expect } from 'vitest';
import {
  getTaskTargetRoute,
  getTaskDetailRoute,
  isModpackTask,
  isModpackUpdateTask,
  getModpackTaskInstanceName,
  type TaskState,
  type TaskType,
} from './task';

describe('task route and helper functions', () => {
  it('correctly maps modpack install tasks to downloads modpack tab', () => {
    const task: TaskType = {
      InstallModpack: {
        zip_path: '/path/to/pack.zip',
        instance_name: 'MyModpack',
        is_update: false,
      },
    };
    expect(isModpackTask(task)).toBe(true);
    expect(isModpackUpdateTask(task)).toBe(false);
    expect(getModpackTaskInstanceName(task)).toBe('MyModpack');
    expect(getTaskTargetRoute(task)).toEqual({
      path: '/downloads',
      query: { tab: 'modpack' },
    });
  });

  it('correctly maps modpack update tasks to instance management update_modpack tab', () => {
    const task: TaskType = {
      InstallOnlineModpack: {
        url: 'https://example.com/pack.mrpack',
        instance_name: 'MyUpdatedInstance',
        is_update: true,
      },
    };
    expect(isModpackTask(task)).toBe(true);
    expect(isModpackUpdateTask(task)).toBe(true);
    expect(getModpackTaskInstanceName(task)).toBe('MyUpdatedInstance');
    expect(getTaskTargetRoute(task)).toEqual({
      path: '/instances/MyUpdatedInstance',
      query: { tab: 'update_modpack' },
    });
  });

  it('correctly maps mod installation tasks to instance mods tab', () => {
    const task: TaskType = {
      InstallMod: {
        source: 'curseforge',
        project_id: '12345',
        mod_name: 'JEI',
        instance_id: 'inst-1',
        target_dir: '/mods',
        download_url: 'https://example.com/mod.jar',
        file_id: '999',
        keep_both: false,
      },
    };
    expect(isModpackTask(task)).toBe(false);
    expect(getModpackTaskInstanceName(task)).toBe(null);
    expect(getTaskTargetRoute(task)).toEqual({
      path: '/instances/inst-1',
      query: { tab: 'mods' },
    });
  });

  it('correctly maps vanilla install task to downloads instance tab', () => {
    const task: TaskType = {
      InstallVanilla: {
        version_id: '1.20.1',
        version_json_url: 'https://example.com',
      },
    };
    expect(getTaskTargetRoute(task)).toEqual({
      path: '/downloads',
      query: { tab: 'instance', install_version: '1.20.1' },
    });
  });

  it('correctly maps loader tasks to instance settings tab', () => {
    const installTask: TaskType = {
      InstallLoaderToInstance: {
        instance_id: 'inst-1',
        mc_version: '1.20.1',
        loader_type: 'Fabric',
        loader_version: '0.16.9',
      },
    };
    expect(getTaskTargetRoute(installTask)).toEqual({
      path: '/instances/inst-1',
      query: { tab: 'settings' },
    });

    const switchTask: TaskType = {
      SwitchInstanceLoaderVersion: {
        instance_id: 'inst-2',
        mc_version: '1.20.1',
        loader_type: 'Forge',
        old_loader_version: '47.2.0',
        new_loader_version: '47.3.0',
      },
    };
    expect(getTaskTargetRoute(switchTask)).toEqual({
      path: '/instances/inst-2',
      query: { tab: 'settings' },
    });

    const uninstallTask: TaskType = {
      UninstallInstanceLoader: {
        instance_id: 'inst-3',
        mc_version: '1.20.1',
        old_loader_type: 'Fabric',
      },
    };
    expect(getTaskTargetRoute(uninstallTask)).toEqual({
      path: '/instances/inst-3',
      query: { tab: 'settings' },
    });
  });

  it('returns null for generic tasks with no specific target', () => {
    const task: TaskType = {
      Generic: {
        name: 'Random job',
      },
    };
    expect(getTaskTargetRoute(task)).toBe(null);
  });
});

function createTaskState(
  id: string,
  status: TaskState['status'],
  task_type: TaskType
): TaskState {
  return {
    id,
    status,
    task_type,
    progress: {
      current: 0,
      total: 100,
      step: 1,
      total_steps: 1,
      detail: '',
      speed: 0,
      remaining_files: 0,
      sub_tasks: [],
    },
    error: null,
    created_at: 0,
    updated_at: 0,
    auto_clear: false,
  };
}

describe('getTaskDetailRoute', () => {
  it('allows active or tracked un-dismissed modpack tasks and hides dismissed or untracked terminal ones', () => {
    const activeModpack = createTaskState('mp-active', 'Running', {
      InstallOnlineModpack: { url: 'https://example.com/pack.mrpack', instance_name: 'PackOne', is_update: false },
    });
    expect(getTaskDetailRoute(activeModpack)).toEqual({
      path: '/downloads',
      query: { tab: 'modpack' },
    });

    const completedModpack = createTaskState('mp-done', 'Completed', {
      InstallModpack: { zip_path: '/pack.zip', instance_name: 'PackOne', is_update: false },
    });
    expect(getTaskDetailRoute(completedModpack)).toBeNull();
    expect(
      getTaskDetailRoute(completedModpack, {
        isTrackedDownloadsModpackTask: (id) => id === 'mp-done',
        isModpackTaskDismissed: () => false,
      })
    ).toEqual({
      path: '/downloads',
      query: { tab: 'modpack' },
    });
    expect(
      getTaskDetailRoute(completedModpack, {
        isTrackedDownloadsModpackTask: (id) => id === 'mp-done',
        isModpackTaskDismissed: (id) => id === 'mp-done',
      })
    ).toBeNull();

    const updateModpack = createTaskState('mp-up', 'Completed', {
      InstallOnlineModpack: { url: 'https://example.com/pack.mrpack', instance_name: 'Server Pack', is_update: true },
    });
    expect(
      getTaskDetailRoute(updateModpack, {
        isTrackedInstanceUpdateTask: (name, id) => name === 'Server Pack' && id === 'mp-up',
      })
    ).toEqual({
      path: '/instances/Server%20Pack',
      query: { tab: 'update_modpack' },
    });
    expect(
      getTaskDetailRoute(updateModpack, {
        isTrackedInstanceUpdateTask: (name, id) => name === 'Server Pack' && id === 'mp-up',
        isModpackTaskDismissed: (id) => id === 'mp-up',
      })
    ).toBeNull();
  });

  it('allows non-dependency active or tracked un-dismissed instance install tasks', () => {
    const activeFabric = createTaskState('inst-fabric', 'Running', {
      InstallFabric: { mc_version: '1.21.4', fabric_version: '0.16.10', custom_instance_name: 'Fabric-1.21.4' },
    });
    expect(getTaskDetailRoute(activeFabric)).toEqual({
      path: '/downloads',
      query: { tab: 'instance', task_id: 'inst-fabric' },
    });

    const completedForge = createTaskState('inst-forge', 'Completed', {
      InstallForge: { mc_version: '1.20.1', loader_version: '47.3.0', loader_type: 'forge', custom_instance_name: 'Forge' },
    });
    expect(getTaskDetailRoute(completedForge)).toBeNull();
    expect(
      getTaskDetailRoute(completedForge, {
        isTrackedInstanceInstallTask: (id) => id === 'inst-forge',
      })
    ).toEqual({
      path: '/downloads',
      query: { tab: 'instance', task_id: 'inst-forge' },
    });
    expect(
      getTaskDetailRoute(completedForge, {
        isTrackedInstanceInstallTask: (id) => id === 'inst-forge',
        isInstanceTaskDismissed: (id) => id === 'inst-forge',
      })
    ).toBeNull();

    const dependencyVanilla = createTaskState('dep-vanilla', 'Running', {
      InstallVanilla: { version_id: '1.21.4', version_json_url: 'https://example.com', is_dependency: true },
    });
    expect(getTaskDetailRoute(dependencyVanilla)).toBeNull();
  });

  it('returns null for tasks without a dedicated task detail view', () => {
    const tasks: TaskState[] = [
      createTaskState('t-mod', 'Running', {
        InstallMod: {
          source: 'modrinth',
          project_id: '1',
          mod_name: 'Sodium',
          instance_id: 'inst-1',
          target_dir: '/mods',
          download_url: 'https://example.com/sodium.jar',
          file_id: '2',
          keep_both: false,
        },
      }),
      createTaskState('t-rp', 'Completed', {
        InstallResourcepack: {
          source: 'modrinth',
          project_id: '1',
          pack_name: 'Faithful',
          instance_id: 'inst-1',
          target_dir: '/resourcepacks',
          download_url: 'https://example.com/rp.zip',
          file_id: '2',
        },
      }),
      createTaskState('t-sp', 'Running', {
        InstallShaderpack: {
          source: 'modrinth',
          project_id: '1',
          pack_name: 'BSL',
          instance_id: 'inst-1',
          target_dir: '/shaderpacks',
          download_url: 'https://example.com/bsl.zip',
          file_id: '2',
        },
      }),
      createTaskState('t-world', 'Running', {
        InstallWorld: {
          source: 'curseforge',
          project_id: '1',
          pack_name: 'Skyblock',
          instance_id: 'inst-1',
          target_dir: '/saves',
          download_url: 'https://example.com/world.zip',
          file_id: '2',
        },
      }),
      createTaskState('t-dp', 'Running', {
        InstallDatapack: {
          source: 'modrinth',
          project_id: '1',
          pack_name: 'Terralith',
          instance_id: 'inst-1',
          target_dir: '/datapacks',
          download_url: 'https://example.com/dp.zip',
          file_id: '2',
        },
      }),
      createTaskState('t-preset', 'Running', {
        InstallPreset: { preset_name: 'PvP', instance_id: 'inst-1' },
      }),
      createTaskState('t-loader-add', 'Running', {
        InstallLoaderToInstance: { instance_id: 'inst-1', mc_version: '1.20.1', loader_type: 'Fabric', loader_version: '0.16.9' },
      }),
      createTaskState('t-loader-switch', 'Running', {
        SwitchInstanceLoaderVersion: { instance_id: 'inst-1', mc_version: '1.20.1', loader_type: 'Forge', new_loader_version: '47.3.0' },
      }),
      createTaskState('t-loader-remove', 'Running', {
        UninstallInstanceLoader: { instance_id: 'inst-1', mc_version: '1.20.1', old_loader_type: 'Fabric' },
      }),
      createTaskState('t-generic', 'Running', {
        Generic: { name: 'Background cleanup' },
      }),
    ];

    for (const task of tasks) {
      expect(getTaskDetailRoute(task)).toBeNull();
    }
  });
});
