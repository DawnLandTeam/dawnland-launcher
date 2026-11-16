import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount, flushPromises } from '@vue/test-utils';
import ResourceTranslationEditModal, {
  type TranslationEditTarget,
} from './ResourceTranslationEditModal.vue';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

describe('ResourceTranslationEditModal', () => {
  const target: TranslationEditTarget = {
    category: 'ShaderPack',
    instanceId: '1.21.1',
    relPath: 'shaderpacks/ComplementaryReimagined_r5.9.3 + EuphoriaPatches_1.10.5.zip',
    filename: 'ComplementaryReimagined_r5.9.3 + EuphoriaPatches_1.10.5.zip',
    source: null,
    projectId: null,
    slug: null,
    rawName: 'ComplementaryReimagined_r5.9.3 + EuphoriaPatches_1.10.5.zip',
    currentZhName: null,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  function mountModal(open = true, editTarget: TranslationEditTarget | null = target) {
    return mount(ResourceTranslationEditModal, {
      props: {
        open,
        target: editTarget,
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

  it('renders only local custom translation controls without community submission option', async () => {
    const wrapper = mountModal(true, target);
    await flushPromises();

    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('社区词库');
  });

  it('saves custom translation locally without submitFeedback field', async () => {
    const mockResult = {
      zhName: '互补光影重制版',
      zhShortName: '互补光影重制版',
      zhSummary: null,
      wikiId: null,
      trustLevel: 4,
      sourceTag: 'user',
    };
    vi.mocked(invoke).mockResolvedValueOnce(mockResult);

    const wrapper = mountModal(true, target);
    await flushPromises();

    const input = wrapper.find('input');
    await input.setValue('互补光影重制版');

    const buttons = wrapper.findAll('button');
    const saveButton = buttons.find((b) => b.text().includes('保存'));
    expect(saveButton).toBeDefined();
    await saveButton!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('set_custom_resource_translation', {
      category: 'ShaderPack',
      source: null,
      projectId: null,
      slug: null,
      rawName: 'ComplementaryReimagined_r5.9.3 + EuphoriaPatches_1.10.5.zip',
      zhName: '互补光影重制版',
      instanceId: '1.21.1',
      relPath: 'shaderpacks/ComplementaryReimagined_r5.9.3 + EuphoriaPatches_1.10.5.zip',
    });

    const invokePayload = vi.mocked(invoke).mock.calls[0][1] as Record<string, unknown>;
    expect('submitFeedback' in invokePayload).toBe(false);

    expect(wrapper.emitted('saved')).toEqual([[mockResult]]);
    expect(wrapper.emitted('update:open')).toEqual([[false]]);
  });
});
