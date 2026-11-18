import { describe, it, expect, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { ref } from 'vue';
import TaskDetailModal from './TaskDetailModal.vue';
import type { TaskState, TaskType } from '../types/task';
import { useModpackInstallState } from '../composables/useModpackInstallState';
import { useInstanceInstallState } from '../composables/useInstanceInstallState';

const mockPush = vi.fn();
const mockCloseTaskDetail = vi.fn();
const mockToggleTaskCenter = vi.fn();
const mockTasks = ref<TaskState[]>([]);
const mockSelectedTaskId = ref<string | null>(null);
const mockIsTaskDetailOpen = ref(true);
const mockIsTaskCenterOpen = ref(true);

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
      te: () => false,
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
    tasks: mockTasks,
    selectedTaskId: mockSelectedTaskId,
    isTaskDetailOpen: mockIsTaskDetailOpen,
    isTaskCenterOpen: mockIsTaskCenterOpen,
    closeTaskDetail: mockCloseTaskDetail,
    toggleTaskCenter: mockToggleTaskCenter,
    cancelTask: vi.fn(),
    forceCancelTask: vi.fn(),
  }),
}));

vi.mock('../composables/useLogStore', () => ({
  useLogStore: () => ({
    getTaskErrorLogs: () => '',
  }),
}));

function makeTask(id: string, status: TaskState['status'], task_type: TaskType, speed = 0): TaskState {
  return {
    id,
    status,
    task_type,
    progress: {
      current: 50,
      total: 100,
      step: 1,
      total_steps: 1,
      detail: 'Working',
      speed,
      remaining_files: 1,
      sub_tasks: [],
    },
    error: null,
    created_at: 0,
    updated_at: 0,
    auto_clear: false,
  };
}

function mountModal() {
  return mount(TaskDetailModal, {
    global: {
      mocks: {
        $t: (key: string, defaultOrParams?: string | Record<string, unknown>) =>
          typeof defaultOrParams === 'string' ? defaultOrParams : key,
      },
      stubs: {
        AiAnalysisPanel: true,
      },
    },
  });
}

describe('TaskDetailModal', () => {
  const modpackState = useModpackInstallState();
  const instanceState = useInstanceInstallState();

  beforeEach(() => {
    vi.clearAllMocks();
    modpackState.clearState();
    instanceState.clearState();
    mockTasks.value = [];
    mockSelectedTaskId.value = null;
    mockIsTaskDetailOpen.value = true;
    mockIsTaskCenterOpen.value = true;
  });

  it('renders header status icon and action buttons in non-overlapping flex layout', async () => {
    const task = makeTask(
      'fabric-1',
      'Running',
      {
        InstallFabric: {
          mc_version: '26.3',
          fabric_version: '0.20.1',
          custom_instance_name: 'Fabric-26.3-0.20.1',
        },
      },
      1024 * 512
    );
    mockTasks.value = [task];
    mockSelectedTaskId.value = task.id;

    const wrapper = mountModal();
    const modal = wrapper.find('.task-detail-modal');
    expect(modal.exists()).toBe(true);

    const header = modal.find(':scope > div:first-child');
    expect(header.classes()).not.toContain('relative');
    expect(header.find('.absolute').exists()).toBe(false);
    expect(header.find('.pr-6').exists()).toBe(false);

    const title = header.find('h3');
    expect(title.classes()).toContain('flex-1');
    expect(title.classes()).toContain('min-w-0');

    const navBtn = header.find('button[title="Go to Task Page"]');
    const closeBtn = header.find('button[title="Close"]');
    expect(navBtn.exists()).toBe(true);
    expect(closeBtn.exists()).toBe(true);

    // Status icon and action buttons must not share the same container
    expect(navBtn.element.parentElement?.querySelector('svg.w-5.h-5')).toBeNull();

    await navBtn.trigger('click');
    expect(mockPush).toHaveBeenCalledWith({
      path: '/downloads',
      query: { tab: 'instance', task_id: 'fabric-1' },
    });
    expect(mockCloseTaskDetail).toHaveBeenCalled();
    expect(mockToggleTaskCenter).toHaveBeenCalled();
  });

  it('shows navigation buttons only for active or un-dismissed detail-page tasks', async () => {
    const modpackTask = makeTask('mp-1', 'Completed', {
      InstallModpack: { zip_path: '/pack.zip', instance_name: 'PackA', is_update: false },
    });
    mockTasks.value = [modpackTask];
    mockSelectedTaskId.value = modpackTask.id;

    const wrapper = mountModal();
    // Untracked terminal task should not show jump buttons
    expect(wrapper.find('button[title="Go to Task Page"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('View Page');

    // Tracked un-dismissed modpack task shows jump buttons
    modpackState.setDownloadsModpackTaskId('mp-1');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('button[title="Go to Task Page"]').exists()).toBe(true);
    expect(wrapper.text()).toContain('View Page');

    // Once dismissed, jump buttons disappear reactively
    modpackState.dismissTask('mp-1');
    await wrapper.vm.$nextTick();
    expect(wrapper.find('button[title="Go to Task Page"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('View Page');
  });

  it('hides navigation buttons for non-detail-page and dependency tasks', async () => {
    const modTask = makeTask('mod-1', 'Running', {
      InstallMod: {
        source: 'curseforge',
        project_id: '123',
        mod_name: 'JEI',
        instance_id: 'inst-1',
        target_dir: '/mods',
        download_url: 'https://example.com/jei.jar',
        file_id: '456',
        keep_both: false,
      },
    });
    const depTask = makeTask('dep-1', 'Running', {
      InstallVanilla: {
        version_id: '1.21.4',
        version_json_url: 'https://example.com/1.21.4.json',
        is_dependency: true,
      },
    });
    const genericTask = makeTask('gen-1', 'Completed', {
      Generic: { name: 'Cleanup' },
    });

    for (const t of [modTask, depTask, genericTask]) {
      mockTasks.value = [t];
      mockSelectedTaskId.value = t.id;
      const wrapper = mountModal();
      expect(wrapper.find('button[title="Go to Task Page"]').exists()).toBe(false);
      expect(wrapper.text()).not.toContain('View Page');
      expect(wrapper.find('button[title="Close"]').exists()).toBe(true);
    }
  });
});
