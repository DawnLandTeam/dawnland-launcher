import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { ref } from 'vue';
import InstanceInstallTab from './InstanceInstallTab.vue';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {})
}));

vi.mock('vue-i18n', async (importOriginal) => {
  const actual: any = await importOriginal();
  return {
    ...actual,
    useI18n: () => ({
      t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
    })
  };
});

vi.mock('../../i18n', () => ({
  default: {
    global: {
      t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
    }
  }
}));

vi.mock('../../utils/analytics', () => ({
  trackEvent: vi.fn(),
  getErrorType: vi.fn()
}));

vi.mock('../../composables/useTaskStore', () => ({
  useTaskStore: () => ({
    tasks: ref([])
  })
}));

vi.mock('../../composables/useInstances', () => ({
  useInstances: () => ({
    fetchInstances: vi.fn().mockResolvedValue([])
  })
}));

describe('InstanceInstallTab - Instance Name Autofill & Refresh', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('autofills instance name with base version when no loader is selected', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_vanilla_versions') {
        return [
          { id: '1.20.1', versionType: 'release', url: 'https://example.com/1.20.1.json' }
        ];
      }
      return [];
    });

    const wrapper = mount(InstanceInstallTab, {
      props: {
        initialVersion: '1.20.1'
      },
      global: {
        mocks: {
          $t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
        },
        stubs: {
          InstallCard: true,
          TaskDetailView: true
        }
      }
    });

    await new Promise(resolve => setTimeout(resolve, 50));
    await wrapper.vm.$nextTick();

    const input = wrapper.find('input');
    expect((input.element as HTMLInputElement).value).toBe('1.20.1');
  });

  it('refreshes instance name when mod loader is selected, changed, or removed', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_vanilla_versions') {
        return [
          { id: '1.20.1', versionType: 'release', url: 'https://example.com/1.20.1.json' }
        ];
      }
      if (cmd === 'get_fabric_loaders') {
        return { stable: ['0.16.10', '0.15.11'], unstable: [] };
      }
      if (cmd === 'get_forge_loaders') {
        return { versions: [{ version: '47.3.0', mcVersion: '1.20.1', installerUrl: '' }] };
      }
      return [];
    });

    const wrapper = mount(InstanceInstallTab, {
      global: {
        mocks: {
          $t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
        },
        stubs: {
          InstallCard: true,
          TaskDetailView: true
        }
      }
    });

    await new Promise(resolve => setTimeout(resolve, 50));
    const vm = wrapper.vm as any;

    // 1. Select version 1.20.1
    vm.selectVersionAndNext('1.20.1');
    await wrapper.vm.$nextTick();
    expect(vm.customInstanceName).toBe('1.20.1');

    // 2. Select Fabric loader
    await vm.loadFabricLoaders();
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Fabric).toBe('0.16.10');
    expect(vm.customInstanceName).toBe('Fabric-1.20.1-0.16.10');

    // 3. Change Fabric version
    vm.selectedComponents.Fabric = '0.15.11';
    await wrapper.vm.$nextTick();
    expect(vm.customInstanceName).toBe('Fabric-1.20.1-0.15.11');

    // 4. Remove Fabric (uncheck / remove loader)
    vm.removeComponent('Fabric');
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Fabric).toBe(null);
    expect(vm.customInstanceName).toBe('1.20.1');

    // 5. Select Forge loader
    await vm.loadForgeLoaders();
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Forge).toBe('47.3.0');
    expect(vm.customInstanceName).toBe('Forge-1.20.1-47.3.0');

    // 6. Go to step 3 and verify input value
    vm.goToStep3();
    await wrapper.vm.$nextTick();
    const input = wrapper.find('input');
    expect((input.element as HTMLInputElement).value).toBe('Forge-1.20.1-47.3.0');
  });

  it('preserves manually customized instance name when mod loader changes', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_vanilla_versions') {
        return [
          { id: '1.20.1', versionType: 'release', url: 'https://example.com/1.20.1.json' }
        ];
      }
      if (cmd === 'get_fabric_loaders') {
        return { stable: ['0.16.10'], unstable: [] };
      }
      if (cmd === 'get_forge_loaders') {
        return { versions: [{ version: '47.3.0', mcVersion: '1.20.1', installerUrl: '' }] };
      }
      return [];
    });

    const wrapper = mount(InstanceInstallTab, {
      global: {
        mocks: {
          $t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
        },
        stubs: {
          InstallCard: true,
          TaskDetailView: true
        }
      }
    });

    await new Promise(resolve => setTimeout(resolve, 50));
    const vm = wrapper.vm as any;

    vm.selectVersionAndNext('1.20.1');
    await vm.loadFabricLoaders();
    vm.goToStep3();
    await wrapper.vm.$nextTick();

    expect(vm.customInstanceName).toBe('Fabric-1.20.1-0.16.10');

    // User manually edits the name
    const input = wrapper.find('input');
    await input.setValue('My-Custom-Instance');
    expect(vm.isNameManuallyEdited).toBe(true);
    expect(vm.customInstanceName).toBe('My-Custom-Instance');

    // User goes back to Step 2 and changes to Forge
    vm.goBackToStep2();
    vm.removeComponent('Fabric');
    await vm.loadForgeLoaders();
    await wrapper.vm.$nextTick();

    // Custom name must be preserved
    expect(vm.customInstanceName).toBe('My-Custom-Instance');

    // Back in Step 3, custom name is still preserved
    vm.goToStep3();
    await wrapper.vm.$nextTick();
    const step3Input = wrapper.find('input');
    expect((step3Input.element as HTMLInputElement).value).toBe('My-Custom-Instance');

    // If user clears the input and blurs, it restores auto-generated name
    await step3Input.setValue('');
    await step3Input.trigger('blur');
    expect(vm.isNameManuallyEdited).toBe(false);
    expect(vm.customInstanceName).toBe('Forge-1.20.1-47.3.0');
  });

  it('discards pending loader response when Minecraft version is changed before response arrives', async () => {
    let resolveFabricLoaders: (val: any) => void;
    const fabricPromise = new Promise((resolve) => {
      resolveFabricLoaders = resolve;
    });

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_vanilla_versions') {
        return [
          { id: '1.20.1', versionType: 'release', url: 'https://example.com/1.20.1.json' },
          { id: '1.20.4', versionType: 'release', url: 'https://example.com/1.20.4.json' }
        ];
      }
      if (cmd === 'get_fabric_loaders') {
        return fabricPromise;
      }
      return [];
    });

    const wrapper = mount(InstanceInstallTab, {
      global: {
        mocks: {
          $t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
        },
        stubs: {
          InstallCard: true,
          TaskDetailView: true
        }
      }
    });

    await new Promise(resolve => setTimeout(resolve, 50));
    const vm = wrapper.vm as any;

    // 1. Select version 1.20.1
    vm.selectVersionAndNext('1.20.1');
    expect(vm.selectedVersion).toBe('1.20.1');

    // 2. Start loading Fabric loaders for 1.20.1
    const loadPromise = vm.loadFabricLoaders();

    // 3. User switches to version 1.20.4 before response arrives
    vm.selectVersionAndNext('1.20.4');
    expect(vm.selectedVersion).toBe('1.20.4');

    // 4. Old response finally arrives
    resolveFabricLoaders!({ stable: ['0.16.10'], unstable: [] });
    await loadPromise;
    await wrapper.vm.$nextTick();

    // Old loader response must NOT be applied to 1.20.4
    expect(vm.selectedComponents.Fabric).toBeNull();
    expect(vm.customInstanceName).toBe('1.20.4');
  });

  it('synchronizes selectedComponents and selector-bound loader refs and refreshes name', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_vanilla_versions') {
        return [
          { id: '1.20.1', versionType: 'release', url: 'https://example.com/1.20.1.json' }
        ];
      }
      return [];
    });

    const wrapper = mount(InstanceInstallTab, {
      global: {
        mocks: {
          $t: (key: string, defaultOrParams?: any) => typeof defaultOrParams === 'string' ? defaultOrParams : key
        },
        stubs: {
          InstallCard: true,
          TaskDetailView: true
        }
      }
    });

    await new Promise(resolve => setTimeout(resolve, 50));
    const vm = wrapper.vm as any;

    vm.selectVersionAndNext('1.20.1');
    await wrapper.vm.$nextTick();
    expect(vm.customInstanceName).toBe('1.20.1');

    // Directly updating selector-bound selectedFabricLoader
    vm.selectedFabricLoader = '0.15.11';
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Fabric).toBe('0.15.11');
    expect(vm.customInstanceName).toBe('Fabric-1.20.1-0.15.11');

    // Clearing selectedFabricLoader clears selectedComponents.Fabric
    vm.selectedFabricLoader = '';
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Fabric).toBeNull();
    expect(vm.customInstanceName).toBe('1.20.1');

    // Directly updating selector-bound selectedForgeLoader
    vm.selectedForgeLoader = '47.2.0';
    await wrapper.vm.$nextTick();
    expect(vm.selectedComponents.Forge).toBe('47.2.0');
    expect(vm.customInstanceName).toBe('Forge-1.20.1-47.2.0');
  });
});
