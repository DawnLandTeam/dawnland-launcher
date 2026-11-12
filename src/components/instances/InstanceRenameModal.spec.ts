import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import { ref } from 'vue';
import InstanceRenameModal from './InstanceRenameModal.vue';
import { invoke } from '@tauri-apps/api/core';

const mockFetchInstances = vi.fn().mockResolvedValue([]);
const mockFetchAllStats = vi.fn().mockResolvedValue([]);
const mockInstances = ref([
  { id: 'PackA', name: 'PackA', mcVersion: '1.20.1', loaderType: 'Fabric', status: 'ready' as const },
  { id: 'PackB', name: 'PackB', mcVersion: '1.20.1', loaderType: 'Vanilla', status: 'ready' as const },
]);

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
    instances: mockInstances,
    fetchInstances: mockFetchInstances,
  }),
}));

vi.mock('../../composables/useStatistics', () => ({
  useStatistics: () => ({
    fetchAllStats: mockFetchAllStats,
  }),
}));

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

describe('InstanceRenameModal', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  function mountModal(open = true, instanceId = 'PackA') {
    return mount(InstanceRenameModal, {
      props: {
        open,
        instanceId,
        instanceName: instanceId,
      },
      global: {
        stubs: {
          Teleport: true,
        },
        mocks: {
          $t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
            typeof defaultOrParams === 'string' ? defaultOrParams : key,
        },
      },
    });
  }

  it('validates invalid characters and duplicate names (case-insensitive)', async () => {
    const wrapper = mountModal(true, 'PackA');
    await flushPromises();

    const input = wrapper.find('input');
    expect((input.element as HTMLInputElement).value).toBe('PackA');

    // Invalid characters
    await input.setValue('bad/name');
    await wrapper.find('form').trigger('submit');
    await flushPromises();
    expect(invoke).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('Instance name contains invalid characters');

    // Conflict with existing instance PackB (case-insensitive)
    await input.setValue('packb');
    await wrapper.find('form').trigger('submit');
    await flushPromises();
    expect(invoke).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain('An instance with this name already exists');
  });

  it('invokes rename_instance, updates selectedInstanceId, dispatches event, and emits renamed', async () => {
    localStorage.setItem('selectedInstanceId', 'PackA');
    const eventSpy = vi.fn();
    window.addEventListener('instance-renamed', eventSpy);

    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const wrapper = mountModal(true, 'PackA');
    await flushPromises();

    const input = wrapper.find('input');
    await input.setValue('PackRenamed');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('rename_instance', {
      oldId: 'PackA',
      newId: 'PackRenamed',
    });
    expect(localStorage.getItem('selectedInstanceId')).toBe('PackRenamed');
    expect(mockFetchInstances).toHaveBeenCalledWith(true);
    expect(mockFetchAllStats).toHaveBeenCalled();
    expect(eventSpy).toHaveBeenCalledTimes(1);
    expect(wrapper.emitted('renamed')).toEqual([[{ oldId: 'PackA', newId: 'PackRenamed' }]]);
    expect(wrapper.emitted('update:open')).toEqual([[false]]);

    window.removeEventListener('instance-renamed', eventSpy);
  });

  it('allows case-only rename of the same instance', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const wrapper = mountModal(true, 'PackA');
    await flushPromises();

    const input = wrapper.find('input');
    await input.setValue('packa');
    await wrapper.find('form').trigger('submit');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('rename_instance', {
      oldId: 'PackA',
      newId: 'packa',
    });
    expect(wrapper.emitted('renamed')).toEqual([[{ oldId: 'PackA', newId: 'packa' }]]);
  });
});
