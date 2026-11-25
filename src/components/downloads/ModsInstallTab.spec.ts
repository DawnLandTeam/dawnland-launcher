import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import ModsInstallTab from './ModsInstallTab.vue';
import i18n from '../../i18n';
import { invoke } from '@tauri-apps/api/core';
import type { DuplicateModCheckResult, EnrichedModProject } from '../../types/resourceNaming';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('vue-router', () => ({
  useRoute: () => ({
    query: {
      instanceId: 'inst-1',
      source: 'modrinth',
    },
  }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
}));

vi.mock('../../utils/analytics', () => ({
  trackEvent: vi.fn(),
  getErrorType: vi.fn(() => 'unknown'),
}));

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock('../../composables/useInstances', () => ({
  useInstances: () => ({
    fetchInstances: vi.fn().mockResolvedValue([
      {
        id: 'inst-1',
        name: 'Fabric 1.20.1',
        mcVersion: '1.20.1',
        loaderType: 'Fabric',
      },
    ]),
  }),
}));

class MockIntersectionObserver {
  observe() {}
  disconnect() {}
  unobserve() {}
}

const litematicaProject: EnrichedModProject = {
  source: 'modrinth',
  project_id: 'bEpr0Arc',
  slug: 'litematica',
  title: 'Litematica',
  description: 'Client-side schematic mod',
  downloads: 1000000,
  author: 'masa',
  mc_versions: ['1.20.1'],
  loaders: ['fabric'],
};

const modFiles = [
  {
    id: 'file-1',
    filename: 'litematica-fabric-1.20.1-0.15.4.jar',
    version_number: '0.15.4',
    download_url: 'https://cdn.modrinth.com/data/bEpr0Arc/versions/file-1/litematica-fabric-1.20.1-0.15.4.jar',
    dependencies: [],
    mc_versions: ['1.20.1'],
    loaders: ['fabric'],
  },
];

function setupInvokeMock(duplicateResult: DuplicateModCheckResult) {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case 'load_launcher_settings':
        return { resourceNaming: { enableOnlineTranslation: true } };
      case 'get_modrinth_categories':
      case 'get_curseforge_categories':
      case 'get_modrinth_game_versions':
      case 'get_curseforge_game_versions':
      case 'get_modrinth_loaders':
      case 'get_curseforge_loaders':
        return [];
      case 'search_modrinth':
      case 'search_curseforge':
        return [litematicaProject];
      case 'enrich_online_projects':
        return [litematicaProject];
      case 'get_modrinth_mod_files':
        return modFiles;
      case 'check_duplicate_mods':
        return duplicateResult;
      case 'delete_local_mod':
        return undefined;
      case 'task_create':
        return 'task-123';
      default:
        return undefined;
    }
  });
}

async function mountAndOpenInstallDialog() {
  const wrapper = mount(ModsInstallTab, {
    global: {
      plugins: [i18n],
      stubs: {
        Teleport: true,
        DSelect: true,
        DMultiSelect: true,
        DInput: true,
        AddToPresetDialog: true,
      },
    },
  });

  await flushPromises();

  const cards = wrapper.findAll('.cursor-pointer');
  const modCard = cards.find((c) => c.text().includes('Litematica'));
  expect(modCard).toBeDefined();
  await modCard!.trigger('click');
  await flushPromises();

  return wrapper;
}

function findButtonByText(wrapper: ReturnType<typeof mount>, text: string) {
  return wrapper.findAll('button').find((b) => b.text().trim() === text);
}

describe('ModsInstallTab duplicate mod detection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    i18n.global.locale.value = 'zh-CN';
    vi.stubGlobal('IntersectionObserver', MockIntersectionObserver);
  });

  it('invokes check_duplicate_mods with expected request and installs directly when no duplicates exist', async () => {
    setupInvokeMock({
      duplicates: [],
      hasExactFilenameConflict: false,
    });

    const wrapper = await mountAndOpenInstallDialog();
    const installBtn = findButtonByText(wrapper, '安装到选中实例');
    expect(installBtn).toBeDefined();
    await installBtn!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('check_duplicate_mods', {
      request: {
        versionId: 'inst-1',
        source: 'modrinth',
        projectId: 'bEpr0Arc',
        slug: 'litematica',
        title: 'Litematica',
        targetFilename: 'litematica-fabric-1.20.1-0.15.4.jar',
      },
    });

    expect(invoke).toHaveBeenCalledWith('task_create', {
      taskType: 'install-mod',
      payload: expect.objectContaining({
        instance_id: 'inst-1',
        project_id: 'bEpr0Arc',
        keep_both: false,
        slug: 'litematica',
        original_filename: 'litematica-fabric-1.20.1-0.15.4.jar',
      }),
    });
    expect(wrapper.text()).not.toContain('检测到重复模组');
  });

  it('shows duplicate dialog and hides keepBoth button when exact filename conflicts', async () => {
    setupInvokeMock({
      duplicates: [
        {
          filename: 'litematica-fabric-1.20.1-0.15.4.jar',
          enabled: true,
          modId: 'litematica',
          name: 'Litematica',
          version: '0.15.4',
          originalFilename: 'litematica-fabric-1.20.1-0.15.4.jar',
          matchReason: 'exactFilename',
          exactFilenameConflict: true,
        },
      ],
      hasExactFilenameConflict: true,
    });

    const wrapper = await mountAndOpenInstallDialog();
    const installBtn = findButtonByText(wrapper, '安装到选中实例');
    await installBtn!.trigger('click');
    await flushPromises();

    expect(wrapper.text()).toContain('检测到重复模组');
    expect(wrapper.text()).toContain('litematica-fabric-1.20.1-0.15.4.jar');
    expect(findButtonByText(wrapper, '保留')).toBeUndefined();
    expect(findButtonByText(wrapper, '覆盖')).toBeDefined();
    expect(invoke).not.toHaveBeenCalledWith('task_create', expect.anything());
  });

  it('passes keep_both: true to task_create when clicking Keep Both', async () => {
    setupInvokeMock({
      duplicates: [
        {
          filename: 'litematica-fabric-1.20.1-0.15.3.jar',
          enabled: true,
          modId: 'litematica',
          name: 'Litematica',
          version: '0.15.3',
          originalFilename: 'litematica-fabric-1.20.1-0.15.3.jar',
          matchReason: 'projectId',
          exactFilenameConflict: false,
        },
      ],
      hasExactFilenameConflict: false,
    });

    const wrapper = await mountAndOpenInstallDialog();
    await findButtonByText(wrapper, '安装到选中实例')!.trigger('click');
    await flushPromises();

    const keepBothBtn = findButtonByText(wrapper, '保留');
    expect(keepBothBtn).toBeDefined();
    await keepBothBtn!.trigger('click');
    await flushPromises();

    expect(invoke).not.toHaveBeenCalledWith('delete_local_mod', expect.anything());
    expect(invoke).toHaveBeenCalledWith('task_create', {
      taskType: 'install-mod',
      payload: expect.objectContaining({
        instance_id: 'inst-1',
        project_id: 'bEpr0Arc',
        keep_both: true,
      }),
    });
  });

  it('deletes each duplicate mod and passes keep_both: false when clicking Replace Old', async () => {
    setupInvokeMock({
      duplicates: [
        {
          filename: 'litematica-fabric-1.20.1-0.15.3.jar',
          enabled: false,
          modId: 'litematica',
          name: 'Litematica',
          version: '0.15.3',
          originalFilename: 'litematica-fabric-1.20.1-0.15.3.jar',
          matchReason: 'modId',
          exactFilenameConflict: false,
        },
      ],
      hasExactFilenameConflict: false,
    });

    const wrapper = await mountAndOpenInstallDialog();
    await findButtonByText(wrapper, '安装到选中实例')!.trigger('click');
    await flushPromises();

    const replaceBtn = findButtonByText(wrapper, '覆盖');
    expect(replaceBtn).toBeDefined();
    await replaceBtn!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('delete_local_mod', {
      versionId: 'inst-1',
      filename: 'litematica-fabric-1.20.1-0.15.3.jar',
      isEnabled: false,
    });
    expect(invoke).toHaveBeenCalledWith('task_create', {
      taskType: 'install-mod',
      payload: expect.objectContaining({
        instance_id: 'inst-1',
        project_id: 'bEpr0Arc',
        keep_both: false,
      }),
    });
  });
});
