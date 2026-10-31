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
const pendingModpackQueue = ref<InspectedDropFile[]>([]);

function openModpackInstall(pack: InspectedDropFile) {
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

function processNextQueuedModpack() {
  if (pendingModpackQueue.value.length > 0) {
    const nextPack = pendingModpackQueue.value.shift()!;
    openModpackInstall(nextPack);
  } else {
    pendingModpackFile.value = null;
    showModpackInstallModal.value = false;
  }
}

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
      return t('dropInstall.kindModpack', 'Modpack');
    case 'Mod':
      return t('dropInstall.kindMod', 'Mod');
    case 'ResourcePack':
      return t('dropInstall.kindResourcePack', 'Resource Pack');
    case 'ShaderPack':
      return t('dropInstall.kindShaderPack', 'Shader Pack');
    case 'Datapack':
      return t('dropInstall.kindDatapack', 'Datapack');
    case 'World':
      return t('dropInstall.kindWorld', 'World Save');
    default:
      return t('dropInstall.kindUnknown', 'Unknown File');
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
    return t('dropInstall.inspecting', 'Inspecting dropped files...');
  }
  if (draggedFiles.value.length === 0) {
    return t('dropInstall.dropFilesHere', 'Release mouse to import files');
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
    return t('dropInstall.hintInstallModpack', 'Release to install modpack');
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
  return t('dropInstall.hintSelectInstance', 'Release to select target instance and import');
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

let worldLoadSeq = 0;
async function loadWorldsForInstance(instanceId: string): Promise<string[]> {
  if (!instanceId) {
    worldOptions.value = [];
    selectedTargetWorld.value = '';
    return [];
  }
  const seq = ++worldLoadSeq;
  loadingWorlds.value = true;
  try {
    const saves = await invoke<string[]>('get_instance_saves', { instanceId });
    if (seq !== worldLoadSeq || selectedTargetInstanceId.value !== instanceId) {
      return saves;
    }
    worldOptions.value = saves.map((s) => ({ label: s, value: s }));
    selectedTargetWorld.value = saves[0] || '';
    return saves;
  } catch (err) {
    if (seq === worldLoadSeq) {
      console.error('Failed to load instance saves for datapack drop:', err);
      worldOptions.value = [];
      selectedTargetWorld.value = '';
    }
    return [];
  } finally {
    if (seq === worldLoadSeq) {
      loadingWorlds.value = false;
    }
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

interface IncompatQueueItem {
  issues: ModIncompatIssue[];
  instance: { name: string; mcVersion: string; loaderType: string };
  resolve: (d: IncompatDecision) => void;
}
const incompatQueue: IncompatQueueItem[] = [];

function requestIncompatConfirm(
  issues: ModIncompatIssue[],
  instance: { name: string; mcVersion: string; loaderType: string },
): Promise<IncompatDecision> {
  return new Promise<IncompatDecision>((resolve) => {
    incompatQueue.push({ issues, instance, resolve });
    if (!showIncompatModal.value) {
      processNextIncompat();
    }
  });
}

function processNextIncompat(): void {
  if (incompatQueue.length === 0) {
    showIncompatModal.value = false;
    return;
  }
  const current = incompatQueue[0];
  incompatIssues.value = current.issues;
  incompatInstanceLabel.value = `${current.instance.name} (${current.instance.mcVersion} · ${current.instance.loaderType})`;
  showIncompatModal.value = true;
}

function resolveIncompat(decision: IncompatDecision): void {
  const current = incompatQueue.shift();
  current?.resolve(decision);
  if (incompatQueue.length > 0) {
    processNextIncompat();
  } else {
    showIncompatModal.value = false;
  }
}

function getKindIcon(kind: DetectedFileKind) {
  switch (kind) {
    case 'Mod':
      return Puzzle;
    case 'ResourcePack':
      return Box;
    case 'ShaderPack':
      return Sparkles;
    case 'Datapack':
      return Package;
    case 'World':
      return Globe;
    default:
      return FolderArchive;
  }
}

// Overwrite confirmation modal state
const showOverwriteModal = ref(false);
const overwriteConflicts = ref<{ filename: string; kind: DetectedFileKind }[]>([]);
const overwriteInstanceLabel = ref('');

interface OverwriteQueueItem {
  conflicts: { filename: string; kind: DetectedFileKind }[];
  instanceLabel: string;
  resolve: (decision: boolean) => void;
}
const overwriteQueue: OverwriteQueueItem[] = [];

function requestOverwriteConfirm(
  conflicts: { filename: string; kind: DetectedFileKind }[],
  instanceLabel: string,
): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    overwriteQueue.push({ conflicts, instanceLabel, resolve });
    if (!showOverwriteModal.value) {
      processNextOverwrite();
    }
  });
}

function processNextOverwrite(): void {
  const next = overwriteQueue[0];
  if (!next) {
    showOverwriteModal.value = false;
    return;
  }
  overwriteConflicts.value = next.conflicts;
  overwriteInstanceLabel.value = next.instanceLabel;
  showOverwriteModal.value = true;
}

function resolveOverwrite(decision: boolean): void {
  const current = overwriteQueue.shift();
  current?.resolve(decision);
  if (overwriteQueue.length > 0) {
    processNextOverwrite();
  } else {
    showOverwriteModal.value = false;
  }
}

function describeIncompatReason(issue: ModIncompatIssue, reason: ModIncompatReason): string {
  switch (reason) {
    case 'vanilla':
      return t('dropInstall.incompatVanilla', 'Target instance is Vanilla with no mod loader installed');
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
      t('dropInstall.instanceBusyTitle', 'Instance is busy'),
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
    const report = await importFilesToInstance(instanceId, filesToImport, targetWorld, false);

    const fileExistsSkips = report.skipped.filter((sk) => sk.reason === 'FILE_EXISTS');
    if (fileExistsSkips.length > 0) {
      const conflictFiles = filesToImport.filter((f) =>
        fileExistsSkips.some((sk) => sk.filename === f.filename),
      );
      const shouldOverwrite = await requestOverwriteConfirm(
        conflictFiles.map((f) => ({ filename: f.filename, kind: f.kind })),
        targetInstance?.name || instanceId,
      );
      if (shouldOverwrite) {
        const overwriteReport = await importFilesToInstance(
          instanceId,
          conflictFiles,
          targetWorld,
          true,
        );
        report.skipped = report.skipped.filter(
          (sk) => !conflictFiles.some((cf) => cf.filename === sk.filename),
        );
        report.succeeded.push(...overwriteReport.succeeded);
        report.failed.push(...overwriteReport.failed);
      }
    }

    if (report.succeeded.length > 0) {
      const counts = new Map<DetectedFileKind, number>();
      for (const s of report.succeeded) {
        counts.set(s.kind, (counts.get(s.kind) || 0) + 1);
      }
      const summaryParts: string[] = [];
      for (const [k, c] of counts.entries()) {
        summaryParts.push(`${c} ${getKindLabel(k)}`);
      }
      const summary = summaryParts.join(', ');

      toast.success(
        t('dropInstall.importSuccessTitle', 'Import successful'),
        t('dropInstall.importSuccessDesc', { summary, instance: instanceId }),
      );
    }

    for (const sk of report.skipped) {
      if (sk.reason === 'NO_WORLD_FOUND') {
        toast.error(
          t('dropInstall.noWorldForDatapackTitle', 'Cannot import datapack'),
          t('dropInstall.noWorldForDatapackDesc', { instance: instanceId }),
        );
      }
    }

    if (report.failed.length > 0) {
      toast.error(
        t('dropInstall.importFailedTitle', 'Failed to import some files'),
        report.failed.map((f) => `${f.filename}: ${f.error}`).join('; '),
      );
    }
  } catch (err) {
    toast.error(t('dropInstall.importFailedTitle', 'Import failed'), getErrorMessage(err));
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

  // Always notify if any unsupported files are included
  if (unknownFiles.length > 0) {
    const names = unknownFiles.map((f) => f.filename).join(', ');
    toast.error(
      t('dropInstall.unsupportedTitle', 'Unsupported file type'),
      t('dropInstall.unsupportedDesc', { name: names }),
    );
  }

  if (modpackFiles.length === 0 && resourceFiles.length === 0) {
    return;
  }

  // 1. Handle Modpack files
  if (modpackFiles.length > 0) {
    if (route.path === '/downloads' && route.query.tab === 'modpack') {
      pendingLocalModpackPath.value = modpackFiles[0].path;
      if (modpackFiles.length > 1) {
        toast.info(
          t('dropInstall.multipleModpacksTitle', 'First modpack selected'),
          t('dropInstall.multipleModpacksDesc', { count: modpackFiles.length - 1 }),
        );
      }
    } else {
      pendingModpackQueue.value = modpackFiles.slice(1);
      openModpackInstall(modpackFiles[0]);
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
            t('dropInstall.noWorldForDatapackTitle', 'Cannot import datapack'),
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
        t('dropInstall.noInstancesTitle', 'No instances available'),
        t('dropInstall.noInstancesDesc', 'Please create a game instance first before dropping resources.'),
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
      t('dropInstall.noWorldForDatapackTitle', 'Cannot import datapack'),
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
    modpackNameError.value = t('dropInstall.emptyInstanceName', 'Please enter an instance name');
    return;
  }

  const isSafeDirectoryName = (name: string) => {
    return (
      name.length > 0 &&
      name !== '.' &&
      name !== '..' &&
      !name.includes('..') &&
      !name.includes('/') &&
      !name.includes('\\') &&
      !/[<>:"/\\|?*\x00-\x1F]/.test(name)
    );
  };

  if (!isSafeDirectoryName(trimmedName)) {
    modpackNameError.value = t('dropInstall.invalidInstanceName', 'Instance name contains invalid characters or path separator');
    return;
  }

  await fetchInstances();
  const exists = instances.value.some((i) => i.id.toLowerCase() === trimmedName.toLowerCase());
  if (exists) {
    modpackNameError.value = t('install.instanceAlreadyExists', 'An instance with this name already exists. Please choose another name.');
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
    await fetchInstances(true);
    toast.success(
      t('dropInstall.modpackTaskStartedTitle', 'Modpack installation started'),
      t('dropInstall.modpackTaskStartedDesc', { instance: trimmedName }),
    );
    taskStore.isTaskCenterOpen.value = true;
    if (route.path !== '/instances') {
      router.push('/instances');
    }
    processNextQueuedModpack();
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
              ? t('dropInstall.selectWorldTitle', 'Select target world save')
              : t('dropInstall.selectInstanceTitle', 'Select target instance')
          }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{
            requireWorldSelectionOnly
              ? t('dropInstall.selectWorldDesc', 'Datapacks need to be installed into a specific world. Please select a world:')
              : t('dropInstall.selectInstanceDesc', 'Please select an instance to import the recognized resources into:')
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
          {{ t('dropInstall.targetInstanceLabel', 'Target Instance') }}
        </label>
        <DSelect
          v-model="selectedTargetInstanceId"
          :options="availableInstanceOptions"
          :placeholder="t('home.selectInstancePlaceholder', 'Please select an instance...')"
          searchable
          :search-placeholder="t('instances.search', 'Search instances...')"
          :empty-text="t('instances.empty.title', 'No instances found')"
          class="w-full"
        />
      </div>

      <!-- World Selector (when Datapack is included) -->
      <div v-if="pendingNeedsWorldSelection" class="space-y-1.5">
        <label class="text-xs font-medium text-neutral-700 dark:text-zinc-300">
          {{ t('dropInstall.targetWorldLabel', 'Target World (Datapack)') }}
        </label>
        <DSelect
          v-model="selectedTargetWorld"
          :options="worldOptions"
          :disabled="loadingWorlds || worldOptions.length === 0"
          :placeholder="
            worldOptions.length === 0
              ? t('dropInstall.noWorldOption', 'This instance has no world saves')
              : t('instances.selectWorldToManage', 'Select a world to manage')
          "
          class="w-full"
        />
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <DButton variant="outline" @click="showInstanceSelectModal = false">
          {{ t('common.cancel', 'Cancel') }}
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
          {{ t('dropInstall.confirmImportBtn', 'Import Now') }}
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
          {{ t('dropInstall.modpackModalTitle', 'Install Dropped Modpack') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ pendingModpackFile?.filename }}
        </DialogDescription>
      </div>

      <div class="space-y-1.5">
        <label class="text-xs font-medium text-neutral-700 dark:text-zinc-300">
          {{ t('install.instanceName', 'Instance Name') }}
        </label>
        <DInput
          v-model="modpackInstanceName"
          :placeholder="t('modpacks.defaultInstanceName', 'Enter instance name')"
          @update:model-value="modpackNameError = null"
        />
        <p v-if="modpackNameError" class="text-xs text-red-500 mt-1">
          {{ modpackNameError }}
        </p>
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <DButton variant="outline" @click="showModpackInstallModal = false">
          {{ t('common.cancel', 'Cancel') }}
        </DButton>
        <DButton
          variant="primary"
          :disabled="isStartingModpackInstall || !modpackInstanceName.trim()"
          @click="confirmQuickModpackInstall"
        >
          <Loader2 v-if="isStartingModpackInstall" class="h-4 w-4 animate-spin mr-1.5" />
          {{ t('dropInstall.startModpackInstallBtn', 'Start Install') }}
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
          {{ t('dropInstall.incompatTitle', 'Potentially Incompatible Mods Detected') }}
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
        {{ t('dropInstall.incompatHint', 'Forcing installation of incompatible mods may cause crashes or failure to start.') }}
      </p>

      <div class="flex justify-end gap-2 pt-1">
        <DButton variant="outline" @click="resolveIncompat('cancel')">
          {{ t('common.cancel', 'Cancel') }}
        </DButton>
        <DButton variant="secondary" @click="resolveIncompat('skip')">
          {{ t('dropInstall.incompatSkip', 'Skip Incompatible') }}
        </DButton>
        <DButton variant="danger" @click="resolveIncompat('force')">
          {{ t('dropInstall.incompatForce', 'Import Anyway') }}
        </DButton>
      </div>
    </div>
  </DialogContent>

  <!-- Overwrite Confirmation Modal -->
  <DialogContent
    :open="showOverwriteModal"
    @update:open="(v: boolean) => { if (!v) resolveOverwrite(false); }"
    class="max-w-md"
  >
    <div class="flex flex-col gap-4">
      <div>
        <DialogTitle class="text-lg font-bold flex items-center gap-2">
          <AlertTriangle class="h-5 w-5 text-amber-500" />
          {{ t('dropInstall.overwriteTitle', 'Files Already Exist') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ t('dropInstall.overwriteDesc', { instance: overwriteInstanceLabel }) }}
        </DialogDescription>
      </div>

      <div
        class="max-h-48 overflow-y-auto custom-scrollbar rounded-xl border border-amber-500/30 bg-amber-50/60 dark:bg-amber-500/5 p-2.5 space-y-1.5"
      >
        <div
          v-for="file in overwriteConflicts"
          :key="file.filename"
          class="flex items-center gap-2 text-xs px-2.5 py-1.5 rounded-lg bg-white/80 dark:bg-zinc-900/80"
        >
          <component :is="getKindIcon(file.kind)" class="h-3.5 w-3.5 text-muted-foreground shrink-0" />
          <span class="truncate font-medium" :title="file.filename">
            {{ file.filename }}
          </span>
          <span class="ml-auto text-[10px] text-muted-foreground shrink-0">
            {{ getKindLabel(file.kind) }}
          </span>
        </div>
      </div>

      <p class="text-[11px] text-muted-foreground">
        {{ t('dropInstall.overwriteWarning', 'Overwriting will replace the existing files and cannot be undone.') }}
      </p>

      <div class="flex justify-end gap-2 pt-1">
        <DButton variant="outline" @click="resolveOverwrite(false)">
          {{ t('common.cancel', 'Cancel') }}
        </DButton>
        <DButton variant="danger" @click="resolveOverwrite(true)">
          {{ t('dropInstall.overwriteConfirmBtn', 'Overwrite Existing') }}
        </DButton>
      </div>
    </div>
  </DialogContent>
</template>
