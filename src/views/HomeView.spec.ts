import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils';
import { ref } from 'vue';
import HomeView from './HomeView.vue';
import i18n from '../i18n';
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../composables/useToast';
import {
  launchingInstances,
  jvmSpawnedInstances,
  runningInstances,
  repairingInstances,
} from '../composables/useLaunchState';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({
  confirm: vi.fn().mockResolvedValue(false),
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
}));

vi.mock('../composables/useStatistics', () => ({
  useStatistics: () => ({
    fetchAllStats: vi.fn().mockResolvedValue([]),
    getInstanceStats: vi.fn(() => null),
    formatPlayTime: vi.fn(() => '0.0'),
    error: ref(null),
  }),
}));

vi.mock('../composables/useInstances', () => ({
  useInstances: () => ({
    fetchInstances: vi.fn().mockResolvedValue([
      {
        id: 'ModPack1',
        name: 'ModPack1',
        mcVersion: '1.20.1',
        loaderType: 'Fabric',
        status: 'ready',
      },
    ]),
  }),
}));

vi.mock('../composables/useInstanceIconUpdates', () => ({
  useInstanceIconUpdates: vi.fn(),
}));

vi.mock('../composables/useFileDrop', () => ({
  useFileDrop: () => ({
    setHomeSelectedInstanceId: vi.fn(),
  }),
}));

vi.mock('../composables/useTaskStatusReload', () => ({
  useTaskStatusReload: vi.fn(),
}));

vi.mock('../composables/useGameLogStore', () => ({
  useGameLogStore: () => ({
    gameLogs: ref([]),
    instanceLogs: ref(new Map()),
    init: vi.fn(),
    setActiveInstanceId: vi.fn(),
    clearActiveLogs: vi.fn(),
    renameInstanceLogs: vi.fn(),
    getInstanceLogs: vi.fn(() => []),
  }),
}));

vi.mock('../composables/useToast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

describe("HomeView - Prelaunch Warning Don't Warn & Launch", () => {
  const wrappers: VueWrapper[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    i18n.global.locale.value = 'zh-CN';
    launchingInstances.value.clear();
    jvmSpawnedInstances.value.clear();
    runningInstances.value.clear();
    repairingInstances.value.clear();
  });

  afterEach(() => {
    while (wrappers.length > 0) {
      wrappers.pop()?.unmount();
    }
    document.body.innerHTML = '';
  });

  function mountHome() {
    const wrapper = mount(HomeView, {
      attachTo: document.body,
      global: {
        plugins: [i18n],
        stubs: {
          'router-link': { template: '<a><slot /></a>' },
          DropdownMenu: { template: '<div><slot name="trigger" /><slot /></div>' },
          DropdownMenuItem: { template: '<div><slot /></div>' },
          CrashReportModal: true,
          InstanceIcon: true,
        },
      },
    });
    wrappers.push(wrapper);
    return wrapper;
  }

  function mockBaseInvoke(handlers: Record<string, (args?: unknown) => unknown>) {
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd in handlers) {
        return handlers[cmd](args);
      }
      switch (cmd) {
        case 'get_accounts':
          return [{ id: 'acc-1', username: 'Tester', accountType: 'offline' }];
        case 'fetch_launcher_announcement':
          return { announcements: [] };
        case 'get_instance_config':
          return { showGameLog: false };
        case 'launch_instance':
          return undefined;
        default:
          return undefined;
      }
    });
  }

  it('disables instance prelaunch check, closes modal, and launches when clicking dontWarnAndLaunch', async () => {
    mockBaseInvoke({
      prelaunch_check: () => ({
        modCount: 1,
        warnings: [
          { kind: 'missing_dependency', dependencyId: 'fabric-api', requiredBy: 'Sodium' },
        ],
      }),
      set_instance_prelaunch_check: () => undefined,
    });

    const wrapper = mountHome();
    await flushPromises();

    const playBtn = wrapper.findAll('button').find((b) => b.text().includes('启动'));
    expect(playBtn).toBeDefined();
    await playBtn!.trigger('click');
    await flushPromises();

    expect(document.body.textContent).toContain('启动前检测到问题');
    expect(document.body.textContent).toContain('fabric-api');
    expect(document.body.textContent).toContain('不再提示并启动');
    expect(document.body.textContent).not.toContain('home.dontWarnAndLaunch');

    i18n.global.locale.value = 'en';
    await flushPromises();
    expect(document.body.textContent).toContain("Don't Warn & Launch");

    const dontWarnBtn = Array.from(document.body.querySelectorAll('button')).find((b) =>
      b.textContent?.includes("Don't Warn & Launch"),
    );
    expect(dontWarnBtn).toBeDefined();
    dontWarnBtn!.click();
    await flushPromises();

    expect(invoke).toHaveBeenCalledWith('set_instance_prelaunch_check', {
      versionId: 'ModPack1',
      enabled: false,
    });
    expect(invoke).toHaveBeenCalledWith('launch_instance', {
      versionId: 'ModPack1',
      accountUuid: 'acc-1',
    });
    expect(document.body.textContent).not.toContain('Pre-launch Warning');
  });

  it('shows translated toast error and keeps modal open without launching when set_instance_prelaunch_check fails', async () => {
    mockBaseInvoke({
      prelaunch_check: () => ({
        modCount: 1,
        warnings: [
          { kind: 'missing_dependency', dependencyId: 'fabric-api', requiredBy: 'Sodium' },
        ],
      }),
      set_instance_prelaunch_check: () => {
        throw new Error('Disk read-only');
      },
    });

    const wrapper = mountHome();
    await flushPromises();

    const playBtn = wrapper.findAll('button').find((b) => b.text().includes('启动'));
    await playBtn!.trigger('click');
    await flushPromises();

    const dontWarnBtn = Array.from(document.body.querySelectorAll('button')).find((b) =>
      b.textContent?.includes('不再提示并启动'),
    );
    expect(dontWarnBtn).toBeDefined();
    dontWarnBtn!.click();
    await flushPromises();

    expect(toast.error).toHaveBeenCalledWith('关闭前置检测失败', 'Disk read-only');
    expect(invoke).not.toHaveBeenCalledWith('launch_instance', expect.anything());
    expect(document.body.textContent).toContain('启动前检测到问题');
  });

  it('launches directly without showing warning modal when prelaunch_check returns empty warnings', async () => {
    mockBaseInvoke({
      prelaunch_check: () => ({
        modCount: 0,
        warnings: [],
      }),
    });

    const wrapper = mountHome();
    await flushPromises();

    const playBtn = wrapper.findAll('button').find((b) => b.text().includes('启动'));
    await playBtn!.trigger('click');
    await flushPromises();

    expect(document.body.textContent).not.toContain('启动前检测到问题');
    expect(invoke).toHaveBeenCalledWith('launch_instance', {
      versionId: 'ModPack1',
      accountUuid: 'acc-1',
    });
  });
});
