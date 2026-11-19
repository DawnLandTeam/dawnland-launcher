import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils';
import { ref } from 'vue';
import SettingsView from './SettingsView.vue';
import i18n from '../i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  hasUpdateAvailable,
  globalUpdateInfo,
  isUpdateModalOpen,
  setUpdateAvailable,
  closeUpdateModal,
} from '../composables/useUpdate';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: vi.fn(),
  save: vi.fn(),
}));

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('1.0.0'),
}));

vi.mock('vue-router', () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
  }),
  useRoute: () => ({
    query: {},
  }),
}));

vi.mock('../utils/analytics', () => ({
  trackEvent: vi.fn(),
  getErrorType: vi.fn(() => 'unknown'),
  sanitizeTrackingUrl: vi.fn((url: string) => url),
}));

vi.mock('../composables/useLogStore', () => ({
  useLogStore: () => ({
    globalLogs: ref([]),
    logConfig: ref({ configuredLevel: null, effectiveLevel: 'info', retentionDays: 7 }),
    loadLogConfig: vi.fn().mockResolvedValue(undefined),
    setLogLevel: vi.fn().mockResolvedValue({ configuredLevel: null, effectiveLevel: 'info', retentionDays: 7 }),
    clearDisplayLogs: vi.fn(),
    readLogFile: vi.fn().mockResolvedValue([]),
    listLogFiles: vi.fn().mockResolvedValue([]),
    openLogDirectory: vi.fn().mockResolvedValue(undefined),
    exportDiagnosticLogs: vi.fn().mockResolvedValue(''),
  }),
}));

vi.mock('../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

describe('SettingsView - Update Channel in About Tab', () => {
  const wrappers: VueWrapper[] = [];
  const alertMock = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    i18n.global.locale.value = 'zh-CN';
    setUpdateAvailable(null);
    closeUpdateModal();
    vi.stubGlobal('alert', alertMock);
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'preview_resource_template') {
        return { rendered: 'preview.jar', fallbackRendered: 'preview.jar', warnings: [] };
      }
      return undefined;
    });
  });

  afterEach(() => {
    while (wrappers.length > 0) {
      wrappers.pop()?.unmount();
    }
    document.body.innerHTML = '';
    vi.unstubAllGlobals();
  });

  function mountSettings() {
    const wrapper = mount(SettingsView, {
      global: {
        plugins: [i18n],
      },
    });
    wrappers.push(wrapper);
    return wrapper;
  }

  async function switchToAboutTab(wrapper: VueWrapper) {
    const aboutTab = wrapper
      .findAll('[role="tab"]')
      .find((tab) => tab.text().includes('关于') || tab.text().includes('About'));
    expect(aboutTab).toBeDefined();
    await aboutTab!.trigger('click');
    await flushPromises();
  }

  async function selectUpdateChannelOption(wrapper: VueWrapper, optionLabel: string) {
    const container = wrapper.find('[data-testid="about-update-channel"]');
    expect(container.exists()).toBe(true);
    const trigger = container.find('button');
    await trigger.trigger('click');
    await flushPromises();

    const options = Array.from(document.body.querySelectorAll('[data-dselect-dropdown] [role="option"]'));
    const target = options.find((el) => el.textContent?.includes(optionLabel));
    expect(target).toBeDefined();
    (target as HTMLElement).click();
    await flushPromises();
  }

  it('moves update channel out of General tab and renders it in About tab with zh/en i18n', async () => {
    const wrapper = mountSettings();
    await flushPromises();

    expect(wrapper.find('[data-testid="about-update-channel"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('更新通道');

    await switchToAboutTab(wrapper);

    const channelRow = wrapper.find('[data-testid="about-update-channel"]');
    expect(channelRow.exists()).toBe(true);
    expect(channelRow.text()).toContain('更新通道');
    expect(channelRow.text()).toContain('正式版 (推荐)');

    i18n.global.locale.value = 'en';
    await flushPromises();

    expect(channelRow.text()).toContain('Update Channel');
    expect(channelRow.text()).toContain('Stable (Recommended)');
    expect(channelRow.text()).not.toContain('settings.about.updateChannelTitle');
  });

  it('persists channel change, clears stale red dot, and silently re-checks update availability without opening modal or alert', async () => {
    localStorage.setItem('updateChannel', 'prerelease');
    setUpdateAvailable({ version: '1.1.0-rc.1', body: 'RC notes' });

    let resolveCheck!: (value: { version: string; body: string } | null) => void;
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'preview_resource_template') {
        return Promise.resolve({ rendered: 'preview.jar', fallbackRendered: 'preview.jar', warnings: [] });
      }
      if (cmd === 'check_launcher_update') {
        return new Promise((resolve) => {
          resolveCheck = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const wrapper = mountSettings();
    await flushPromises();
    await switchToAboutTab(wrapper);

    expect(hasUpdateAvailable.value).toBe(true);

    await selectUpdateChannelOption(wrapper, '正式版 (推荐)');

    // Old red dot is cleared immediately before background check settles
    expect(localStorage.getItem('updateChannel')).toBe('stable');
    expect(hasUpdateAvailable.value).toBe(false);
    expect(globalUpdateInfo.value).toBeNull();
    expect(isUpdateModalOpen.value).toBe(false);
    expect(alertMock).not.toHaveBeenCalled();

    expect(invoke).toHaveBeenCalledWith('check_launcher_update', { channel: 'stable' });

    resolveCheck({ version: '1.0.5', body: 'Stable release' });
    await flushPromises();

    expect(hasUpdateAvailable.value).toBe(true);
    expect(globalUpdateInfo.value?.version).toBe('1.0.5');
    expect(isUpdateModalOpen.value).toBe(false);
    expect(alertMock).not.toHaveBeenCalled();
  });

  it('ignores stale silent check responses and keeps red dot clear on failure', async () => {
    localStorage.setItem('updateChannel', 'stable');

    let resolvePrerelease!: (value: { version: string; body: string } | null) => void;
    let resolveStable!: (value: { version: string; body: string } | null) => void;

    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === 'preview_resource_template') {
        return Promise.resolve({ rendered: 'preview.jar', fallbackRendered: 'preview.jar', warnings: [] });
      }
      if (cmd === 'check_launcher_update') {
        const channel = (args as { channel?: string } | undefined)?.channel;
        if (channel === 'prerelease') {
          return new Promise((resolve) => {
            resolvePrerelease = resolve;
          });
        }
        return new Promise((resolve) => {
          resolveStable = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const wrapper = mountSettings();
    await flushPromises();
    await switchToAboutTab(wrapper);

    await selectUpdateChannelOption(wrapper, '预览版');
    await selectUpdateChannelOption(wrapper, '正式版 (推荐)');

    resolveStable(null);
    await flushPromises();
    expect(hasUpdateAvailable.value).toBe(false);

    // Late response from prerelease must not override current stable state
    resolvePrerelease({ version: '1.2.0-rc.1', body: 'Late RC' });
    await flushPromises();
    expect(hasUpdateAvailable.value).toBe(false);
    expect(globalUpdateInfo.value).toBeNull();

    // Error during silent check must not show alert or open modal
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'check_launcher_update') {
        throw new Error('network offline');
      }
      return undefined;
    });

    await selectUpdateChannelOption(wrapper, '预览版');
    await flushPromises();

    expect(hasUpdateAvailable.value).toBe(false);
    expect(isUpdateModalOpen.value).toBe(false);
    expect(alertMock).not.toHaveBeenCalled();
  });

  it('uses the updated channel when manually checking for updates', async () => {
    localStorage.setItem('updateChannel', 'stable');
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'preview_resource_template') {
        return { rendered: 'preview.jar', fallbackRendered: 'preview.jar', warnings: [] };
      }
      if (cmd === 'check_launcher_update') {
        return { version: '1.2.0-rc.1', body: 'Preview build' };
      }
      return undefined;
    });

    const wrapper = mountSettings();
    await flushPromises();
    await switchToAboutTab(wrapper);

    await selectUpdateChannelOption(wrapper, '预览版');
    await flushPromises();

    vi.mocked(invoke).mockClear();

    const checkBtn = wrapper
      .findAll('button')
      .find((btn) => btn.text().includes('检查更新') || btn.text().includes('Check for Updates'));
    expect(checkBtn).toBeDefined();
    await checkBtn!.trigger('click');
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('check_launcher_update', { channel: 'prerelease' });
    expect(isUpdateModalOpen.value).toBe(true);
    expect(globalUpdateInfo.value?.version).toBe('1.2.0-rc.1');
  });
});
