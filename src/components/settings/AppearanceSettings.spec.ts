import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils';
import { invoke } from '@tauri-apps/api/core';
import AppearanceSettings from './AppearanceSettings.vue';
import i18n from '../../i18n';
import { toast } from '../../composables/useToast';
import { __resetBackgroundStoreForTesting } from '../../composables/useBackgroundSettings';
import type { LauncherBackgroundState } from '../../types/background';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${path}`),
}));

vi.mock('../../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

describe('AppearanceSettings.vue', () => {
  const wrappers: VueWrapper[] = [];

  const defaultState: LauncherBackgroundState = {
    mode: 'default',
    imageFile: null,
    imagePath: null,
    fileSizeBytes: null,
    blurPx: 4,
    overlayOpacity: null,
    fit: 'cover',
    updatedAt: 0,
    revision: 0,
    resetEpoch: 0,
  };

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    __resetBackgroundStoreForTesting();
    i18n.global.locale.value = 'zh-CN';

    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_launcher_background') {
        return { ...defaultState };
      }
      return undefined;
    });
  });

  afterEach(() => {
    while (wrappers.length > 0) {
      wrappers.pop()?.unmount();
    }
    document.body.innerHTML = '';
  });

  function mountAppearance() {
    const wrapper = mount(AppearanceSettings, {
      global: {
        plugins: [i18n],
      },
    });
    wrappers.push(wrapper);
    return wrapper;
  }

  it('renders bilingual labels and default background state', async () => {
    const wrapper = mountAppearance();
    await flushPromises();

    expect(wrapper.text()).toContain('自定义启动器背景图');
    expect(wrapper.find('[data-testid="background-source-badge"]').text()).toContain('默认背景');
    expect(wrapper.find('[data-testid="overlay-auto-badge"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="reset-background-btn"]').attributes('disabled')).toBeDefined();

    i18n.global.locale.value = 'en';
    await flushPromises();

    expect(wrapper.text()).toContain('Custom Launcher Background');
    expect(wrapper.find('[data-testid="background-source-badge"]').text()).toContain(
      'Default Wallpaper',
    );
  });

  it('imports a custom background through backend native picker and ignores cancelled selection', async () => {
    const customResult: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-999.png',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-999.png',
      fileSizeBytes: 512 * 1024,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 123,
      revision: 1,
      resetEpoch: 0,
    };

    let pickCallCount = 0;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_launcher_background') {
        return { ...defaultState };
      }
      if (cmd === 'pick_and_set_launcher_background') {
        pickCallCount += 1;
        return pickCallCount === 1 ? null : customResult;
      }
      return undefined;
    });

    const wrapper = mountAppearance();
    await flushPromises();

    // First click: user cancels dialog -> backend returns null -> no toast
    await wrapper.find('[data-testid="choose-background-btn"]').trigger('click');
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith('pick_and_set_launcher_background', {
      dialogTitle: '选择启动器背景图片',
      filterName: '图片文件',
    });
    expect(toast.success).not.toHaveBeenCalled();

    // Second click: user picks an image -> backend imports & returns custom state
    await wrapper.find('[data-testid="choose-background-btn"]').trigger('click');
    await flushPromises();

    expect(toast.success).toHaveBeenCalledWith('背景图已更新');
    expect(wrapper.find('[data-testid="background-source-badge"]').text()).toContain('自定义背景');
    expect(wrapper.find('[data-testid="background-source-badge"]').text()).toContain('PNG');
    expect(
      wrapper.find('[data-testid="reset-background-btn"]').attributes('disabled'),
    ).toBeUndefined();
  });

  it('shows localized error toasts when backend rejects invalid image format or oversized dimensions', async () => {
    let pickAttempt = 0;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_launcher_background') {
        return { ...defaultState };
      }
      if (cmd === 'pick_and_set_launcher_background') {
        pickAttempt += 1;
        if (pickAttempt === 1) {
          throw { code: 'INVALID_IMAGE_FORMAT', message: 'Unsupported format' };
        }
        throw { code: 'IMAGE_DIMENSIONS_TOO_LARGE', message: 'Too large dimensions' };
      }
      return undefined;
    });

    const wrapper = mountAppearance();
    await flushPromises();

    await wrapper.find('[data-testid="choose-background-btn"]').trigger('click');
    await flushPromises();

    expect(toast.error).toHaveBeenCalledWith(
      '设置背景图失败',
      '不支持的图片格式或文件已损坏，请选择 PNG、JPG、WebP 或 GIF 图片。',
    );

    await wrapper.find('[data-testid="choose-background-btn"]').trigger('click');
    await flushPromises();

    expect(toast.error).toHaveBeenCalledWith(
      '设置背景图失败',
      '所选图片分辨率或动画帧数过大，请缩小尺寸后重试。',
    );
  });

  it('updates fill mode, blur, overlay opacity, resets auto overlay, and resets to default', async () => {
    const customState: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-1.jpg',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-1.jpg',
      fileSizeBytes: 1024,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 50,
      revision: 1,
      resetEpoch: 0,
    };

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'get_launcher_background') {
        return { ...customState };
      }
      if (cmd === 'update_launcher_background_style') {
        const p = args as { blurPx: number; overlayOpacity: number | null; fit: 'cover' | 'contain' | 'fill' };
        return {
          ...customState,
          blurPx: p.blurPx,
          overlayOpacity: p.overlayOpacity,
          fit: p.fit,
          updatedAt: 60,
          revision: 2,
        };
      }
      if (cmd === 'reset_launcher_background') {
        return { ...defaultState, updatedAt: 70, revision: 3, resetEpoch: 1 };
      }
      return undefined;
    });

    const wrapper = mountAppearance();
    await flushPromises();

    // Change fit via DToggleGroup radio button
    const containRadio = wrapper
      .findAll('[role="radio"]')
      .find((btn) => btn.text().includes('完整显示'));
    expect(containRadio).toBeDefined();
    await containRadio!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('update_launcher_background_style', {
      blurPx: 4,
      overlayOpacity: null,
      fit: 'contain',
      expectedResetEpoch: 0,
    });

    // Change blur slider
    const blurSlider = wrapper.find('[data-testid="background-blur-slider"]');
    await blurSlider.setValue('12');
    await blurSlider.trigger('change');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('update_launcher_background_style', {
      blurPx: 12,
      overlayOpacity: null,
      fit: 'contain',
      expectedResetEpoch: 0,
    });

    // Change overlay slider -> manual value removes auto badge and exposes reset-auto button
    const overlaySlider = wrapper.find('[data-testid="background-overlay-slider"]');
    await overlaySlider.setValue('50');
    await overlaySlider.trigger('change');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('update_launcher_background_style', {
      blurPx: 12,
      overlayOpacity: 50,
      fit: 'contain',
      expectedResetEpoch: 0,
    });
    expect(wrapper.find('[data-testid="reset-auto-overlay-btn"]').exists()).toBe(true);

    // Click reset auto overlay
    await wrapper.find('[data-testid="reset-auto-overlay-btn"]').trigger('click');
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith('update_launcher_background_style', {
      blurPx: 12,
      overlayOpacity: null,
      fit: 'contain',
      expectedResetEpoch: 0,
    });
    expect(wrapper.find('[data-testid="overlay-auto-badge"]').exists()).toBe(true);

    // Click reset to default
    await wrapper.find('[data-testid="reset-background-btn"]').trigger('click');
    await flushPromises();
    expect(invoke).toHaveBeenCalledWith('reset_launcher_background', { resetStyle: true });
    expect(toast.success).toHaveBeenCalledWith('已恢复默认背景');
  });
});
