<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import {
  UploadCloud,
  Puzzle,
  Box,
  Sparkles,
  Package,
  Globe,
  FolderArchive,
  Loader2,
  AlertTriangle,
} from '@lucide/vue';
import { DialogContent, DialogTitle, DialogDescription } from './ui/dialog';
import DButton from './ui/DButton.vue';
import DInput from './ui/DInput.vue';
import DSelect from './ui/DSelect.vue';
import { useFileDrop } from '../composables/useFileDrop';
import { useInstances } from '../composables/useInstances';
import { useTaskStore } from '../composables/useTaskStore';
import { toast } from '../composables/useToast';
import { getErrorMessage } from '../utils/error';
import { findIncompatibleMods, type ModIncompatIssue, type ModIncompatReason } from '../utils/modCompat';
import type { DetectedFileKind, InspectedDropFile } from '../types';

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const { instances, fetchInstances } = useInstances();
const taskStore = useTaskStore();

const {
  isDragging,
  isInspecting,
  draggedFiles,
  hoveredInstanceId,
  activeDatapackWorld,
  homeSelectedInstanceId,
  showInstanceSelectModal,
  pendingResourceFiles,
  preselectedInstanceId,
  requireWorldSelectionOnly,
  showModpackInstallModal,
  pendingModpackFile,
  pendingLocalModpackPath,
  initFileDropListener,
  destroyFileDropListener,
  importFilesToInstance,
} = useFileDrop();

// Target instance & world selection modal state
const selectedTargetInstanceId = ref<string>('');
const worldOptions = ref<{ label: string; value: string }[]>([]);
const selectedTargetWorld = ref<string>('');
const loadingWorlds = ref(false);
const isImporting = ref(false);

// Quick modpack install modal state
const modpackInstanceName = ref<string>('');
const modpackNameError = ref<string | null>(null);
const isStartingModpackInstall = ref(false);

const availableInstanceOptions = computed(() =>
  instances.value
    .filter((i) => !i.isInstalling && !i.isUpdating)
    .map((i) => ({
      label: i.name,
      sublabel: `${i.mcVersion} · ${i.loaderType}`,
      value: i.id,
      iconPath: i.iconPath ?? null,
      showInstanceIcon: true,
    })),
);

const pendingNeedsWorldSelection = computed(() =>
  pendingResourceFiles.value.some((f) => f.kind === 'Datapack'),
);

function getKindLabel(kind: DetectedFileKind): string {
  switch (kind) {
    case 'Modpack':
      return t('dropInstall.kindModpack', '整合包');
    case 'Mod':
      return t('dropInstall.kindMod', '模组');
    case 'ResourcePack':
      return t('dropInstall.kindResourcePack', '资源包');
    case 'ShaderPack':
      return t('dropInstall.kindShaderPack', '光影包');
    case 'Datapack':
      return t('dropInstall.kindDatapack', '数据包');
    case 'World':
      return t('dropInstall.kindWorld', '世界存档');
    default:
      return t('dropInstall.kindUnknown', '未知文件');
  }
}

function summarizeFileKinds(files: InspectedDropFile[]): string {
  const counts = new Map<DetectedFileKind, number>();
  for (const f of files) {
    counts.set(f.kind, (counts.get(f.kind) || 0) + 1);
  }
  const parts: string[] = [];
  for (const [kind, count] of counts.entries()) {
    if (kind === 'Modpack' && count === 1 && files[0]?.displayName) {
      parts.push(`${getKindLabel(kind)}: ${files[0].displayName}`);
    } else {
      parts.push(`${count} ${getKindLabel(kind)}`);
    }
  }
  return parts.join('、');
}

const dragSummaryText = computed(() => {
  if (isInspecting.value) {
    return t('dropInstall.inspecting', '正在识别拖入的文件...');
  }
  if (draggedFiles.value.length === 0) {
    return t('dropInstall.dropFilesHere', '松开鼠标以导入文件');
  }
  return summarizeFileKinds(draggedFiles.value);
});

const activeRouteInstanceId = computed<string | null>(() => {
  if (route.path.startsWith('/instances/') && typeof route.params.id === 'string') {
    return route.params.id;
  }
  return null;
});

const isDraggingModpack = computed(() =>
  draggedFiles.value.some((f) => f.kind === 'Modpack'),
);

const showWindowOverlay = computed(() => {
  if (!isDragging.value) return false;
  if (hoveredInstanceId.value && !isDraggingModpack.value) return false;
  return true;
});

const showWindowDropFrame = computed(() => {
  if (route.path === '/instances' && !isDraggingModpack.value) return false;
  return true;
});

const dropTargetHint = computed(() => {
  if (isDraggingModpack.value) {
    return t('dropInstall.hintInstallModpack', '松开以安装整合包');
  }
  if (hoveredInstanceId.value) {
    return t('dropInstall.hintImportToInstance', { instance: hoveredInstanceId.value });
  }
  if (activeRouteInstanceId.value) {
    return t('dropInstall.hintImportToInstance', { instance: activeRouteInstanceId.value });
  }
  if (route.path === '/' && homeSelectedInstanceId.value) {
    return t('dropInstall.hintImportToInstance', { instance: homeSelectedInstanceId.value });
  }
  return t('dropInstall.hintSelectInstance', '松开以选择目标实例并导入');
});

function inferFallbackKindFromActiveTab(filename: string): DetectedFileKind | null {
  if (!activeRouteInstanceId.value) return null;
  const tab = typeof route.query.tab === 'string' ? route.query.tab : '';
  const lower = filename.toLowerCase();
  if (tab === 'mods' && lower.endsWith('.jar')) return 'Mod';
  if (tab === 'resourcepacks' && lower.endsWith('.zip')) return 'ResourcePack';
  if (tab === 'shaders' && lower.endsWith('.zip')) return 'ShaderPack';
  if (tab === 'datapacks' && lower.endsWith('.zip')) return 'Datapack';
  return null;
}

async function loadWorldsForInstance(instanceId: string): Promise<string[]> {
  if (!instanceId) {
    worldOptions.value = [];
    selectedTargetWorld.value = '';
    return [];
  }
  loadingWorlds.value = true;
  try {
    const saves = await invoke<string[]>('get_instance_saves', { instanceId });
    worldOptions.value = saves.map((s) => ({ label: s, value: s }));
    selectedTargetWorld.value = saves[0] || '';
    return saves;
  } catch (err) {
    console.error('Failed to load instance saves for datapack drop:', err);
    worldOptions.value = [];
    selectedTargetWorld.value = '';
    return [];
  } finally {
    loadingWorlds.value = false;
  }
}

watch(selectedTargetInstanceId, async (newId) => {
  if (showInstanceSelectModal.value && pendingNeedsWorldSelection.value && newId) {
    await loadWorldsForInstance(newId);
  }
});

// Incompatible mod confirmation modal state
type IncompatDecision = 'force' | 'skip' | 'cancel';
const showIncompatModal = ref(false);
const incompatIssues = ref<ModIncompatIssue[]>([]);
const incompatInstanceLabel = ref('');
let incompatResolver: ((d: IncompatDecision) => void) | null = null;

function requestIncompatConfirm(
  issues: ModIncompatIssue[],
  instance: { name: string; mcVersion: string; loaderType: string },
): Promise<IncompatDecision> {
  incompatIssues.value = issues;
  incompatInstanceLabel.value = `${instance.name} (${instance.mcVersion} · ${instance.loaderType})`;
  showIncompatModal.value = true;
  return new Promise<IncompatDecision>((resolve) => {
    incompatResolver = resolve;
  });
}

function resolveIncompat(decision: IncompatDecision): void {
  showIncompatModal.value = false;
  const r = incompatResolver;
  incompatResolver = null;
  r?.(decision);
}

function describeIncompatReason(issue: ModIncompatIssue, reason: ModIncompatReason): string {
  switch (reason) {
    case 'vanilla':
      return t('dropInstall.incompatVanilla', '目标实例为原版，未安装模组加载器');
    case 'loader':
      return t('dropInstall.incompatLoader', { loader: issue.modLoader ?? '?' });
    case 'mcVersion':
      return t('dropInstall.incompatMcVersion', { required: issue.requiredMcVersion ?? '?' });
  }
}

async function executeImportAndNotify(
  instanceId: string,
  files: InspectedDropFile[],
  targetWorld?: string,
): Promise<void> {
  if (instances.value.length === 0) {
    await fetchInstances();
  }
  const targetInstance = instances.value.find((i) => i.id === instanceId);
  if (targetInstance && (targetInstance.isInstalling || targetInstance.isUpdating)) {
    toast.error(
      t('dropInstall.instanceBusyTitle', '实例繁忙'),
      t('dropInstall.instanceBusyDesc', { instance: instanceId }),
    );
    return;
  }

  // Ask for confirmation before importing incompatible mods
  let filesToImport = files;
  if (targetInstance) {
    const issues = findIncompatibleMods(files, {
      loaderType: targetInstance.loaderType,
      mcVersion: targetInstance.mcVersion,
    });
    if (issues.length > 0) {
      const decision = await requestIncompatConfirm(issues, targetInstance);
      if (decision === 'cancel') return;
      if (decision === 'skip') {
        const skipPaths = new Set(issues.map((i) => i.file.path));
        filesToImport = files.filter((f) => !skipPaths.has(f.path));
        if (filesToImport.length === 0) return;
      }
    }
  }

  try {
    const report = await importFilesToInstance(instanceId, filesToImport, targetWorld, true);

    if (report.succeeded.length > 0) {
      const counts = new Map<DetectedFileKind, number>();
      for (const s of report.succeeded) {
        counts.set(s.kind, (counts.get(s.kind) || 0) + 1);
      }
      const summaryParts: string[] = [];
      for (const [k, c] of counts.entries()) {
        summaryParts.push(`${c} ${getKindLabel(k)}`);
      }
      const summary = summaryParts.join('、');

      toast.success(
        t('dropInstall.importSuccessTitle', '拖拽导入成功'),
        t('dropInstall.importSuccessDesc', { summary, instance: instanceId }),
      );
    }

    for (const sk of report.skipped) {
      if (sk.reason === 'NO_WORLD_FOUND') {
        toast.error(
          t('dropInstall.noWorldForDatapackTitle', '无法导入数据包'),
          t('dropInstall.noWorldForDatapackDesc', { instance: instanceId }),
        );
      }
    }

    if (report.failed.length > 0) {
      toast.error(
        t('dropInstall.importFailedTitle', '部分文件导入失败'),
        report.failed.map((f) => `${f.filename}: ${f.error}`).join('; '),
      );
    }
  } catch (err) {
    toast.error(t('dropInstall.importFailedTitle', '导入失败'), getErrorMessage(err));
  }
}

async function onFilesDropped(
  rawFiles: InspectedDropFile[],
  targetCardInstanceId: string | null,
): Promise<void> {
  // Apply active-tab fallback for ambiguous zip files if inside InstanceManagementView
  const files: InspectedDropFile[] = rawFiles.map((f) => {
    if (f.kind === 'Unknown') {
      const fallback = inferFallbackKindFromActiveTab(f.filename);
      if (fallback) {
        return { ...f, kind: fallback };
      }
    }
    return f;
  });

  const modpackFiles = files.filter((f) => f.kind === 'Modpack');
  const resourceFiles = files.filter((f) => f.kind !== 'Modpack' && f.kind !== 'Unknown');
  const unknownFiles = files.filter((f) => f.kind === 'Unknown');

  if (modpackFiles.length === 0 && resourceFiles.length === 0) {
    if (unknownFiles.length > 0) {
      toast.error(
        t('dropInstall.unsupportedTitle', '不支持的文件类型'),
        t('dropInstall.unsupportedDesc', { name: unknownFiles[0].filename }),
      );
    }
    return;
  }

  // 1. Handle Modpack files
  if (modpackFiles.length > 0) {
    const pack = modpackFiles[0];
    if (route.path === '/downloads' && route.query.tab === 'modpack') {
      pendingLocalModpackPath.value = pack.path;
    } else {
      pendingModpackFile.value = pack;
      const defaultName = (
        pack.displayName || pack.filename.replace(/\.(zip|mrpack)$/i, '')
      )
        .replace(/[<>:"/\\|?*]/g, '')
        .trim();
      modpackInstanceName.value = defaultName || 'New_Modpack';
      modpackNameError.value = null;
      showModpackInstallModal.value = true;
    }
  }

  // 2. Handle Resource files (Mod, ResourcePack, ShaderPack, Datapack, World)
  if (resourceFiles.length > 0) {
    if (instances.value.length === 0) {
      await fetchInstances();
    }

    const resolvedInstanceId =
      targetCardInstanceId ||
      activeRouteInstanceId.value ||
      (route.path === '/' && homeSelectedInstanceId.value ? homeSelectedInstanceId.value : null);

    if (resolvedInstanceId) {
      const hasDatapack = resourceFiles.some((f) => f.kind === 'Datapack');
      if (hasDatapack) {
        const currentWorld =
          activeRouteInstanceId.value === resolvedInstanceId &&
          route.query.tab === 'datapacks' &&
          activeDatapackWorld.value
            ? activeDatapackWorld.value
            : '';

        if (currentWorld) {
          await executeImportAndNotify(resolvedInstanceId, resourceFiles, currentWorld);
          return;
        }

        const saves = await loadWorldsForInstance(resolvedInstanceId);
        if (saves.length === 0) {
          toast.error(
            t('dropInstall.noWorldForDatapackTitle', '无法导入数据包'),
            t('dropInstall.noWorldForDatapackDesc', { instance: resolvedInstanceId }),
          );
          const nonDatapacks = resourceFiles.filter((f) => f.kind !== 'Datapack');
          if (nonDatapacks.length > 0) {
            await executeImportAndNotify(resolvedInstanceId, nonDatapacks);
          }
          return;
        }
        if (saves.length === 1) {
          await executeImportAndNotify(resolvedInstanceId, resourceFiles, saves[0]);
          return;
        }

        // Multiple worlds exist -> prompt user to pick world
        pendingResourceFiles.value = resourceFiles;
        preselectedInstanceId.value = resolvedInstanceId;
        selectedTargetInstanceId.value = resolvedInstanceId;
        requireWorldSelectionOnly.value = true;
        showInstanceSelectModal.value = true;
        return;
      }

      await executeImportAndNotify(resolvedInstanceId, resourceFiles);
      return;
    }

    // No target instance in context -> open instance selection modal
    const readyInstances = instances.value.filter((i) => !i.isInstalling && !i.isUpdating);
    if (readyInstances.length === 0) {
      toast.error(
        t('dropInstall.noInstancesTitle', '暂无可用实例'),
        t('dropInstall.noInstancesDesc', '请先创建一个游戏实例后再拖入资源文件。'),
      );
      return;
    }

    pendingResourceFiles.value = resourceFiles;
    requireWorldSelectionOnly.value = false;
    selectedTargetInstanceId.value = readyInstances[0].id;
    preselectedInstanceId.value = '';
    if (resourceFiles.some((f) => f.kind === 'Datapack')) {
      await loadWorldsForInstance(readyInstances[0].id);
    }
    showInstanceSelectModal.value = true;
  }
}

async function confirmInstanceSelectionImport(): Promise<void> {
  if (!selectedTargetInstanceId.value || pendingResourceFiles.value.length === 0) return;
  if (pendingNeedsWorldSelection.value && !selectedTargetWorld.value) {
    toast.error(
      t('dropInstall.noWorldForDatapackTitle', '无法导入数据包'),
      t('dropInstall.noWorldForDatapackDesc', { instance: selectedTargetInstanceId.value }),
    );
    return;
  }

  isImporting.value = true;
  try {
    const targetId = selectedTargetInstanceId.value;
    const files = [...pendingResourceFiles.value];
    const world = pendingNeedsWorldSelection.value ? selectedTargetWorld.value : undefined;
    showInstanceSelectModal.value = false;
    pendingResourceFiles.value = [];
    await executeImportAndNotify(targetId, files, world);
  } finally {
    isImporting.value = false;
  }
}

async function confirmQuickModpackInstall(): Promise<void> {
  if (!pendingModpackFile.value) return;
  const trimmedName = modpackInstanceName.value.trim();
  if (!trimmedName) {
    modpackNameError.value = t('dropInstall.emptyInstanceName', '请输入实例名称');
    return;
  }

  await fetchInstances();
  const exists = instances.value.some((i) => i.id.toLowerCase() === trimmedName.toLowerCase());
  if (exists) {
    modpackNameError.value = t(
      'install.instanceAlreadyExists',
      '已存在同名实例，请更换其他名称。',
    );
    return;
  }

  isStartingModpackInstall.value = true;
  modpackNameError.value = null;
  try {
    await invoke<string>('install_modpack', {
      zipPath: pendingModpackFile.value.path,
      instanceName: trimmedName,
      isUpdate: false,
      projectId: null,
    });
    showModpackInstallModal.value = false;
    pendingModpackFile.value = null;
    await fetchInstances(true);
    toast.success(
      t('dropInstall.modpackTaskStartedTitle', '已开始安装整合包'),
      t('dropInstall.modpackTaskStartedDesc', { instance: trimmedName }),
    );
    taskStore.isTaskCenterOpen.value = true;
    if (route.path !== '/instances') {
      router.push('/instances');
    }
  } catch (err) {
    modpackNameError.value = getErrorMessage(err);
  } finally {
    isStartingModpackInstall.value = false;
  }
}

onMounted(() => {
  void initFileDropListener(onFilesDropped);
});

onUnmounted(() => {
  destroyFileDropListener();
});
</script>

<template>
  <!-- Global Drag Hover Overlay -->
  <Transition
    enter-active-class="transition duration-150 ease-out"
    enter-from-class="opacity-0"
    enter-to-class="opacity-100"
    leave-active-class="transition duration-150 ease-in"
    leave-from-class="opacity-100"
    leave-to-class="opacity-0"
  >
    <div
      v-if="showWindowOverlay"
      class="fixed inset-0 z-[90] pointer-events-none flex flex-col items-center justify-end pb-10 p-3"
    >
      <!-- Highlight frame around window -->
      <div
        v-if="showWindowDropFrame"
        class="absolute inset-2 rounded-2xl border-2 border-dashed border-primary/60 bg-primary/[0.03] transition-all"
      />

      <!-- Bottom floating capsule -->
      <div
        class="relative z-10 flex items-center gap-4 rounded-2xl border border-primary/30 bg-white/95 dark:bg-zinc-900/95 px-6 py-4 shadow-2xl backdrop-blur-md max-w-lg"
      >
        <div
          class="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-primary/15 text-primary"
        >
          <Loader2 v-if="isInspecting" class="h-6 w-6 animate-spin" />
          <UploadCloud v-else class="h-6 w-6" />
        </div>
        <div class="flex flex-col min-w-0">
          <span class="text-sm font-bold text-neutral-900 dark:text-zinc-100 truncate">
            {{ dragSummaryText }}
          </span>
          <span class="text-xs font-medium text-primary mt-0.5 truncate">
            {{ dropTargetHint }}
          </span>
        </div>
      </div>
    </div>
  </Transition>

  <!-- Target Instance & World Selection Modal -->
  <DialogContent
    :open="showInstanceSelectModal"
    @update:open="showInstanceSelectModal = $event"
    class="max-w-md !overflow-visible"
  >
    <div class="flex flex-col gap-4">
      <div>
        <DialogTitle class="text-lg font-bold flex items-center gap-2">
          <UploadCloud class="h-5 w-5 text-primary" />
          {{
            requireWorldSelectionOnly
              ? t('dropInstall.selectWorldTitle', '选择目标存档')
              : t('dropInstall.selectInstanceTitle', '选择要导入的目标实例')
          }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{
            requireWorldSelectionOnly
              ? t('dropInstall.selectWorldDesc', '数据包需要安装到具体的存档目录中，请选择目标世界：')
              : t('dropInstall.selectInstanceDesc', '已识别以下拖入的资源文件，请选择要导入的实例：')
          }}
        </DialogDescription>
      </div>

      <!-- File list preview -->
      <div
        class="max-h-36 overflow-y-auto custom-scrollbar rounded-xl border border-neutral-200 dark:border-zinc-800 bg-neutral-50/70 dark:bg-zinc-950/60 p-2.5 space-y-1.5"
      >
        <div
          v-for="file in pendingResourceFiles"
          :key="file.path"
          class="flex items-center justify-between gap-2 text-xs px-2 py-1 rounded-lg bg-white/80 dark:bg-zinc-900/80"
        >
          <div class="flex items-center gap-2 min-w-0">
            <Puzzle v-if="file.kind === 'Mod'" class="h-3.5 w-3.5 text-primary shrink-0" />
            <Box v-else-if="file.kind === 'ResourcePack'" class="h-3.5 w-3.5 text-emerald-500 shrink-0" />
            <Sparkles v-else-if="file.kind === 'ShaderPack'" class="h-3.5 w-3.5 text-amber-500 shrink-0" />
            <Package v-else-if="file.kind === 'Datapack'" class="h-3.5 w-3.5 text-indigo-500 shrink-0" />
            <Globe v-else-if="file.kind === 'World'" class="h-3.5 w-3.5 text-blue-500 shrink-0" />
            <FolderArchive v-else class="h-3.5 w-3.5 text-muted-foreground shrink-0" />
            <span class="truncate font-medium" :title="file.filename">{{ file.filename }}</span>
          </div>
          <span
            class="shrink-0 rounded-md bg-primary/10 text-primary px-1.5 py-0.5 text-[10px] font-semibold"
          >
            {{ getKindLabel(file.kind) }}
          </span>
        </div>
      </div>

      <!-- Instance Selector -->
      <div v-if="!requireWorldSelectionOnly" class="space-y-1.5">
        <label class="text-xs font-medium text-neutral-700 dark:text-zinc-300">
          {{ t('dropInstall.targetInstanceLabel', '目标实例') }}
        </label>
        <DSelect
          v-model="selectedTargetInstanceId"
          :options="availableInstanceOptions"
          :placeholder="t('home.selectInstancePlaceholder', '请选择一个实例...')"
          searchable
          :search-placeholder="t('instances.search', '搜索实例...')"
          :empty-text="t('instances.empty.title', '未找到实例')"
          class="w-full"
        />
      </div>

      <!-- World Selector (when Datapack is included) -->
      <div v-if="pendingNeedsWorldSelection" class="space-y-1.5">
        <label class="text-xs font-medium text-neutral-700 dark:text-zinc-300">
          {{ t('dropInstall.targetWorldLabel', '目标存档 (数据包)') }}
        </label>
        <DSelect
          v-model="selectedTargetWorld"
          :options="worldOptions"
          :disabled="loadingWorlds || worldOptions.length === 0"
          :placeholder="
            worldOptions.length === 0
              ? t('dropInstall.noWorldOption', '该实例暂无存档')
              : t('instances.selectWorldToManage', '选择要管理的存档')
          "
          class="w-full"
        />
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <DButton variant="outline" @click="showInstanceSelectModal = false">
          {{ t('common.cancel', '取消') }}
        </DButton>
        <DButton
          variant="primary"
          :disabled="
            isImporting ||
            !selectedTargetInstanceId ||
            (pendingNeedsWorldSelection && !selectedTargetWorld)
          "
          @click="confirmInstanceSelectionImport"
        >
          {{ t('dropInstall.confirmImportBtn', '立即导入') }}
        </DButton>
      </div>
    </div>
  </DialogContent>

  <!-- Quick Modpack Install Modal -->
  <DialogContent
    :open="showModpackInstallModal"
    @update:open="showModpackInstallModal = $event"
    class="max-w-md"
  >
    <div class="flex flex-col gap-4">
      <div>
        <DialogTitle class="text-lg font-bold flex items-center gap-2">
          <Package class="h-5 w-5 text-emerald-600" />
          {{ t('dropInstall.modpackModalTitle', '安装拖入的整合包') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ pendingModpackFile?.filename }}
        </DialogDescription>
      </div>

      <div class="space-y-1.5">
        <label class="text-xs font-medium text-neutral-700 dark:text-zinc-300">
          {{ t('install.instanceName', '实例名称') }}
        </label>
        <DInput
          v-model="modpackInstanceName"
          :placeholder="t('modpacks.defaultInstanceName', '输入实例名称')"
          @update:model-value="modpackNameError = null"
        />
        <p v-if="modpackNameError" class="text-xs text-red-500 mt-1">
          {{ modpackNameError }}
        </p>
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <DButton variant="outline" @click="showModpackInstallModal = false">
          {{ t('common.cancel', '取消') }}
        </DButton>
        <DButton
          variant="primary"
          :disabled="isStartingModpackInstall || !modpackInstanceName.trim()"
          @click="confirmQuickModpackInstall"
        >
          <Loader2 v-if="isStartingModpackInstall" class="h-4 w-4 animate-spin mr-1.5" />
          {{ t('dropInstall.startModpackInstallBtn', '开始安装') }}
        </DButton>
      </div>
    </div>
  </DialogContent>

  <!-- Incompatible Mod Confirmation Modal -->
  <DialogContent
    :open="showIncompatModal"
    @update:open="(v: boolean) => { if (!v) resolveIncompat('cancel'); }"
    class="max-w-md"
  >
    <div class="flex flex-col gap-4">
      <div>
        <DialogTitle class="text-lg font-bold flex items-center gap-2">
          <AlertTriangle class="h-5 w-5 text-amber-500" />
          {{ t('dropInstall.incompatTitle', '检测到可能不兼容的模组') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ t('dropInstall.incompatDesc', { instance: incompatInstanceLabel }) }}
        </DialogDescription>
      </div>

      <div
        class="max-h-48 overflow-y-auto custom-scrollbar rounded-xl border border-amber-500/30 bg-amber-50/60 dark:bg-amber-500/5 p-2.5 space-y-1.5"
      >
        <div
          v-for="issue in incompatIssues"
          :key="issue.file.path"
          class="flex flex-col gap-1 text-xs px-2.5 py-1.5 rounded-lg bg-white/80 dark:bg-zinc-900/80"
        >
          <div class="flex items-center gap-2 min-w-0">
            <Puzzle class="h-3.5 w-3.5 text-amber-500 shrink-0" />
            <span class="truncate font-medium" :title="issue.file.filename">
              {{ issue.file.displayName || issue.file.filename }}
            </span>
          </div>
          <ul class="pl-5 space-y-0.5 text-[11px] text-amber-700 dark:text-amber-400 list-disc">
            <li v-for="reason in issue.reasons" :key="reason">
              {{ describeIncompatReason(issue, reason) }}
            </li>
          </ul>
        </div>
      </div>

      <p class="text-[11px] text-muted-foreground">
        {{ t('dropInstall.incompatHint', '强制导入不兼容的模组可能导致游戏无法启动或崩溃。') }}
      </p>

      <div class="flex justify-end gap-2 pt-1">
        <DButton variant="outline" @click="resolveIncompat('cancel')">
          {{ t('common.cancel', '取消') }}
        </DButton>
        <DButton variant="secondary" @click="resolveIncompat('skip')">
          {{ t('dropInstall.incompatSkip', '跳过不兼容项') }}
        </DButton>
        <DButton variant="danger" @click="resolveIncompat('force')">
          {{ t('dropInstall.incompatForce', '仍然导入') }}
        </DButton>
      </div>
    </div>
  </DialogContent>
</template>
