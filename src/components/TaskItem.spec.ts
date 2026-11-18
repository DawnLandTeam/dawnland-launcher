import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { ref } from 'vue';
import TaskItem from './TaskItem.vue';
import type { TaskState, TaskType } from '../types/task';
import { useModpackInstallState } from '../composables/useModpackInstallState';
import { useInstanceInstallState } from '../composables/useInstanceInstallState';

const mockPush = vi.fn();
const mockOpenTaskDetail = vi.fn();
const mockCloseTaskDetail = vi.fn();
const mockToggleTaskCenter = vi.fn();
const mockIsTaskCenterOpen = ref(true);
const mockIsTaskDetailOpen = ref(false);

vi.mock('vue-router', () => ({
  useRouter: () => ({
    push: mockPush,
  }),
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

vi.mock('../i18n', () => ({
  default: {
    global: {
      t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
        typeof defaultOrParams === 'string' ? defaultOrParams : key,
    },
  },
}));

vi.mock('../composables/useTaskStore', () => ({
  useTaskStore: () => ({
    isTaskCenterOpen: mockIsTaskCenterOpen,
    isTaskDetailOpen: mockIsTaskDetailOpen,
    openTaskDetail: mockOpenTaskDetail,
    closeTaskDetail: mockCloseTaskDetail,
    toggleTaskCenter: mockToggleTaskCenter,
    cancelTask: vi.fn(),
    retryTask: vi.fn(),
  }),
}));

function makeTask(id: string, status: TaskState['status'], task_type: TaskType): TaskState {
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

function mountItem(task: TaskState) {
  return mount(TaskItem, {
    props: { task },
    global: {
      mocks: {
        $t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
          typeof defaultOrParams === 'string' ? defaultOrParams : key,
      },
    },
  });
}

describe('TaskItem', () => {
  const modpackState = useModpackInstallState();
  const instanceState = useInstanceInstallState();

  beforeEach(() => {
    vi.clearAllMocks();
    modpackState.clearState();
    instanceState.clearState();
  });

  it('shows navigation button for active modpack task and navigates cleanly', async () => {
    const task = makeTask('mp-active', 'Running', {
      InstallOnlineModpack: { url: 'https://example.com/p.mrpack', instance_name: 'PackX', is_update: false },
    });
    const wrapper = mountItem(task);

    const navBtn = wrapper.find('button[title="Go to Task Page"]');
    expect(navBtn.exists()).toBe(true);
    await navBtn.trigger('click');

    expect(mockPush).toHaveBeenCalledWith({
      path: '/downloads',
      query: { tab: 'modpack' },
    });
    expect(mockToggleTaskCenter).toHaveBeenCalled();
  });

  it('hides navigation button for dismissed modpack tasks and non-detail-page tasks', () => {
    modpackState.setDownloadsModpackTaskId('mp-done');
    modpackState.dismissTask('mp-done');

    const dismissedModpack = makeTask('mp-done', 'Completed', {
      InstallModpack: { zip_path: '/p.zip', instance_name: 'PackX', is_update: false },
    });
    expect(mountItem(dismissedModpack).find('button[title="Go to Task Page"]').exists()).toBe(false);

    const loaderSwitch = makeTask('loader-1', 'Running', {
      SwitchInstanceLoaderVersion: {
        instance_id: 'inst-1',
        mc_version: '1.20.1',
        loader_type: 'Forge',
        new_loader_version: '47.3.0',
      },
    });
    expect(mountItem(loaderSwitch).find('button[title="Go to Task Page"]').exists()).toBe(false);
  });
});
