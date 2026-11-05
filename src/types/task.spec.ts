import { describe, it, expect } from 'vitest';
import {
  getTaskTargetRoute,
  isModpackTask,
  getModpackTaskInstanceName,
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

  it('returns null for generic tasks with no specific target', () => {
    const task: TaskType = {
      Generic: {
        name: 'Random job',
      },
    };
    expect(getTaskTargetRoute(task)).toBe(null);
  });
});
