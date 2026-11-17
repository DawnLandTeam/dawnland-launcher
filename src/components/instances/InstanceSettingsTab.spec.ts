import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { ref } from 'vue';
import InstanceSettingsTab from './InstanceSettingsTab.vue';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('vue-i18n', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return {
    ...actual,
    useI18n: () => ({
      t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
        typeof defaultOrParams === 'string' ? defaultOrParams : key,
    }),
  };
});

vi.mock('../../i18n', () => ({
  default: {
    global: {
      t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
        typeof defaultOrParams === 'string' ? defaultOrParams : key,
    },
  },
}));

vi.mock('../../composables/useInstances', () => ({
  useInstances: () => ({
    fetchInstances: vi.fn().mockResolvedValue([]),
  }),
}));

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock('../../composables/useLaunchState', () => ({
  repairingInstances: ref(new Set<string>()),
  isInstanceActive: () => false,
}));

describe('InstanceSettingsTab - Prelaunch Mod Dependency Check Switch', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function mockBackendConfig(configOverride: Record<string, unknown> = {}) {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      switch (cmd) {
        case 'get_system_memory':
          return { totalMb: 16384, recommendedMaxMb: 4096 };
        case 'load_launcher_settings':
          return { globalMaxMemory: 4096 };
        case 'scan_local_javas':
          return [];
        case 'get_instance_config':
          return {
            windowWidth: 1280,
            windowHeight: 720,
            windowBehavior: 'keep',
            showGameLog: false,
            ...configOverride,
          };
        case 'get_jvm_tuning_preview':
          return null;
        case 'save_instance_config':
          return undefined;
        default:
          return undefined;
      }
    });
  }

  it('shows the prelaunch check toggle for modded instances and saves changes', async () => {
    mockBackendConfig();

    const wrapper = mount(InstanceSettingsTab, {
      props: {
        instanceId: 'FabricPack',
        instance: {
          id: 'FabricPack',
          name: 'FabricPack',
          mcVersion: '1.20.1',
          loaderType: 'Fabric',
          loaderVersion: '0.16.10',
          status: 'ready',
        },
      },
      global: {
        stubs: {
          ModLoaderInstallDialog: true,
          ModLoaderUninstallDialog: true,
        },
        mocks: {
          $t: (key: string) => key,
        },
      },
    });

    await flushPromises();

    const toggle = wrapper.find('#enablePrelaunchCheck');
    expect(toggle.exists()).toBe(true);
    expect((toggle.element as HTMLInputElement).checked).toBe(true);

    await toggle.setValue(false);
    const saveButton = wrapper.findAll('button').find((b) => b.text().includes('common.save') || b.text().includes('Save'));
    expect(saveButton).toBeDefined();
    await saveButton!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith(
      'save_instance_config',
      expect.objectContaining({
        versionId: 'FabricPack',
        config: expect.objectContaining({
          enablePrelaunchCheck: false,
        }),
      }),
    );
  });

  it('loads false state when enablePrelaunchCheck is disabled in config', async () => {
    mockBackendConfig({ enablePrelaunchCheck: false });

    const wrapper = mount(InstanceSettingsTab, {
      props: {
        instanceId: 'ForgePack',
        instance: {
          id: 'ForgePack',
          name: 'ForgePack',
          mcVersion: '1.20.1',
          loaderType: 'Forge',
          status: 'ready',
        },
      },
      global: {
        stubs: {
          ModLoaderInstallDialog: true,
          ModLoaderUninstallDialog: true,
        },
        mocks: {
          $t: (key: string) => key,
        },
      },
    });

    await flushPromises();

    const toggle = wrapper.find('#enablePrelaunchCheck');
    expect(toggle.exists()).toBe(true);
    expect((toggle.element as HTMLInputElement).checked).toBe(false);
  });

  it('hides the prelaunch check toggle for Vanilla instances', async () => {
    mockBackendConfig();

    const wrapper = mount(InstanceSettingsTab, {
      props: {
        instanceId: 'Vanilla_1_21',
        instance: {
          id: 'Vanilla_1_21',
          name: 'Vanilla_1_21',
          mcVersion: '1.21.1',
          loaderType: 'Vanilla',
          status: 'ready',
        },
      },
      global: {
        stubs: {
          ModLoaderInstallDialog: true,
          ModLoaderUninstallDialog: true,
        },
        mocks: {
          $t: (key: string) => key,
        },
      },
    });

    await flushPromises();

    expect(wrapper.find('#enablePrelaunchCheck').exists()).toBe(false);
  });
});
