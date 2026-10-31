import { ref } from 'vue';
import { toast } from './useToast';
import { getErrorMessage } from '../utils/error';
import i18n from '../i18n';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type {
  DetectedFileKind,
  DropImportItem,
  DropImportReport,
  InspectedDropFile,
} from '../types';

export const INSTANCE_ASSETS_UPDATED_EVENT = 'instance-assets-updated';

export interface InstanceAssetsUpdatedDetail {
  instanceId: string;
  kinds: DetectedFileKind[];
}

export function emitInstanceAssetsUpdated(instanceId: string, kinds: DetectedFileKind[]): void {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(
    new CustomEvent<InstanceAssetsUpdatedDetail>(INSTANCE_ASSETS_UPDATED_EVENT, {
      detail: { instanceId, kinds },
    }),
  );
}

/**
 * Convert Tauri PhysicalPosition pixels into CSS logical pixels for DOM hit-testing.
 */
export function physicalToLogicalPosition(
  pos: { x: number; y: number },
  dpr: number = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1,
): { x: number; y: number } {
  const safeDpr = dpr > 0 && Number.isFinite(dpr) ? dpr : 1;
  return {
    x: pos.x / safeDpr,
    y: pos.y / safeDpr,
  };
}

/**
 * Hit-test the DOM at logical coordinates for an element with `[data-drop-instance-id]`.
 */
export function findHoveredInstanceIdAtPoint(logicalX: number, logicalY: number): string | null {
  if (typeof document === 'undefined' || typeof document.elementFromPoint !== 'function') {
    return null;
  }
  const el = document.elementFromPoint(logicalX, logicalY);
  if (!el) return null;
  const card = el.closest('[data-drop-instance-id]');
  if (!card) return null;
  const id = card.getAttribute('data-drop-instance-id');
  return id && id.trim().length > 0 ? id.trim() : null;
}

// Module-scoped reactive state
const isDragging = ref(false);
const isInspecting = ref(false);
const draggedFiles = ref<InspectedDropFile[]>([]);
const hoveredInstanceId = ref<string | null>(null);
const activeDatapackWorld = ref<string>('');
const homeSelectedInstanceId = ref<string>('');

// Modal & handoff states
const showInstanceSelectModal = ref(false);
const pendingResourceFiles = ref<InspectedDropFile[]>([]);
const preselectedInstanceId = ref<string>('');
const requireWorldSelectionOnly = ref(false);

const showModpackInstallModal = ref(false);
const pendingModpackFile = ref<InspectedDropFile | null>(null);
const pendingLocalModpackPath = ref<string | null>(null);

let unlistenDragDrop: UnlistenFn | null = null;
let inspectSeq = 0;
let isWindowElevatedForDrag = false;

async function setWindowTopmostForDrag(active: boolean, focusAfter = false): Promise<void> {
  try {
    const appWindow = getCurrentWindow();
    if (active) {
      if (!isWindowElevatedForDrag) {
        isWindowElevatedForDrag = true;
        await appWindow.setAlwaysOnTop(true);
      }
    } else {
      if (isWindowElevatedForDrag) {
        isWindowElevatedForDrag = false;
        await appWindow.setAlwaysOnTop(false);
      }
      if (focusAfter) {
        await appWindow.setFocus();
      }
    }
  } catch (err) {
    console.warn('Failed to update window topmost state during drag:', err);
  }
}

export type DropHandlerCallback = (
  files: InspectedDropFile[],
  targetCardInstanceId: string | null,
) => Promise<void>;

let onFilesDroppedCallback: DropHandlerCallback | null = null;

export function useFileDrop() {
  async function inspectPaths(paths: string[]): Promise<InspectedDropFile[]> {
    if (paths.length === 0) return [];
    const currentSeq = ++inspectSeq;
    isInspecting.value = true;
    try {
      const inspected = await invoke<InspectedDropFile[]>('inspect_dropped_files', { paths });
      if (currentSeq === inspectSeq) {
        draggedFiles.value = inspected;
      }
      return inspected;
    } catch (err) {
      console.error('Failed to inspect dropped files:', err);
      if (currentSeq === inspectSeq) {
        draggedFiles.value = [];
        toast.error((i18n.global.t as any)('dropInstall.inspectFailedTitle', 'Failed to inspect dropped files'), getErrorMessage(err));
      }
      return [];
    } finally {
      if (currentSeq === inspectSeq) {
        isInspecting.value = false;
      }
    }
  }

  async function handleDragDropPayload(payload: DragDropEvent): Promise<void> {
    if (payload.type === 'enter') {
      isDragging.value = true;
      void setWindowTopmostForDrag(true);
      const logical = physicalToLogicalPosition(payload.position);
      hoveredInstanceId.value = findHoveredInstanceIdAtPoint(logical.x, logical.y);
      const previewPaths = payload.paths.slice(0, 50);
      await inspectPaths(previewPaths);
    } else if (payload.type === 'over') {
      isDragging.value = true;
      void setWindowTopmostForDrag(true);
      const logical = physicalToLogicalPosition(payload.position);
      hoveredInstanceId.value = findHoveredInstanceIdAtPoint(logical.x, logical.y);
    } else if (payload.type === 'leave') {
      isDragging.value = false;
      hoveredInstanceId.value = null;
      draggedFiles.value = [];
      void setWindowTopmostForDrag(false, false);
    } else if (payload.type === 'drop') {
      const logical = physicalToLogicalPosition(payload.position);
      const droppedOnInstanceId =
        findHoveredInstanceIdAtPoint(logical.x, logical.y) ?? hoveredInstanceId.value;

      isDragging.value = false;
      hoveredInstanceId.value = null;
      void setWindowTopmostForDrag(false, true);

      // Reuse already-inspected files if paths match, otherwise inspect now
      const existing = draggedFiles.value;
      draggedFiles.value = [];

      let filesToProcess: InspectedDropFile[];
      if (
        existing.length === payload.paths.length &&
        existing.every((f, idx) => f.path === payload.paths[idx])
      ) {
        filesToProcess = existing;
      } else {
        filesToProcess = await inspectPaths(payload.paths);
        draggedFiles.value = [];
      }

      if (filesToProcess.length > 0 && onFilesDroppedCallback) {
        await onFilesDroppedCallback(filesToProcess, droppedOnInstanceId);
      }
    }
  }

  async function initFileDropListener(onDrop: DropHandlerCallback): Promise<void> {
    onFilesDroppedCallback = onDrop;
    if (unlistenDragDrop) {
      unlistenDragDrop();
      unlistenDragDrop = null;
    }
    try {
      unlistenDragDrop = await getCurrentWebview().onDragDropEvent((event) => {
        void handleDragDropPayload(event.payload);
      });
    } catch (err) {
      console.warn('Failed to register webview drag-drop listener:', err);
    }
  }

  function destroyFileDropListener(): void {
    if (unlistenDragDrop) {
      unlistenDragDrop();
      unlistenDragDrop = null;
    }
    onFilesDroppedCallback = null;
    isDragging.value = false;
    hoveredInstanceId.value = null;
    draggedFiles.value = [];
    void setWindowTopmostForDrag(false, false);
  }

  async function importFilesToInstance(
    instanceId: string,
    files: InspectedDropFile[],
    targetWorld?: string,
    overwrite = false,
  ): Promise<DropImportReport> {
    const items: DropImportItem[] = files.map((f) => ({
      path: f.path,
      kindOverride: f.kind !== 'Unknown' ? f.kind : null,
    }));

    const report = await invoke<DropImportReport>('import_dropped_files_to_instance', {
      instanceId,
      items,
      targetWorld: targetWorld || null,
      overwrite,
    });

    if (report.succeeded.length > 0) {
      const kinds = Array.from(new Set(report.succeeded.map((s) => s.kind)));
      emitInstanceAssetsUpdated(instanceId, kinds);
    }

    return report;
  }

  function setActiveDatapackWorld(worldName: string | null): void {
    activeDatapackWorld.value = worldName || '';
  }

  function setHomeSelectedInstanceId(instanceId: string): void {
    homeSelectedInstanceId.value = instanceId;
  }

  return {
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
    handleDragDropPayload,
    importFilesToInstance,
    setActiveDatapackWorld,
    setHomeSelectedInstanceId,
  };
}
