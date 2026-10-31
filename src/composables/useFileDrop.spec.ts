import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  physicalToLogicalPosition,
  findHoveredInstanceIdAtPoint,
  emitInstanceAssetsUpdated,
  INSTANCE_ASSETS_UPDATED_EVENT,
  useFileDrop,
} from './useFileDrop';
import type { InspectedDropFile, InstanceAssetsUpdatedDetail } from '../types';

const mockInvoke = vi.fn();
const mockSetAlwaysOnTop = vi.fn().mockResolvedValue(undefined);
const mockSetFocus = vi.fn().mockResolvedValue(undefined);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    setAlwaysOnTop: (...args: unknown[]) => mockSetAlwaysOnTop(...args),
    setFocus: (...args: unknown[]) => mockSetFocus(...args),
  }),
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn().mockResolvedValue(() => {}),
  }),
}));

describe('useFileDrop helpers & state', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    mockInvoke.mockReset();
    mockSetAlwaysOnTop.mockClear();
    mockSetFocus.mockClear();
    const { destroyFileDropListener } = useFileDrop();
    destroyFileDropListener();
    mockSetAlwaysOnTop.mockClear();
    mockSetFocus.mockClear();
  });

  it('converts physical coordinates to logical CSS coordinates using devicePixelRatio', () => {
    expect(physicalToLogicalPosition({ x: 300, y: 150 }, 1.5)).toEqual({
      x: 200,
      y: 100,
    });
    expect(physicalToLogicalPosition({ x: 400, y: 200 }, 2)).toEqual({
      x: 200,
      y: 100,
    });
    // Fallback when dpr is invalid
    expect(physicalToLogicalPosition({ x: 120, y: 80 }, 0)).toEqual({
      x: 120,
      y: 80,
    });
  });

  it('finds [data-drop-instance-id] ancestor via elementFromPoint', () => {
    const card = document.createElement('div');
    card.setAttribute('data-drop-instance-id', 'fabric-1.20.1');
    const innerSpan = document.createElement('span');
    card.appendChild(innerSpan);
    document.body.appendChild(card);

    const originalElementFromPoint = document.elementFromPoint;
    document.elementFromPoint = vi.fn().mockReturnValue(innerSpan);

    try {
      const logical = physicalToLogicalPosition({ x: 100, y: 200 }, 2);
      const id = findHoveredInstanceIdAtPoint(logical.x, logical.y);
      expect(document.elementFromPoint).toHaveBeenCalledWith(50, 100);
      expect(id).toBe('fabric-1.20.1');
    } finally {
      document.elementFromPoint = originalElementFromPoint;
    }
  });

  it('updates drag state and invokes drop callback across enter/over/drop events', async () => {
    const card = document.createElement('div');
    card.setAttribute('data-drop-instance-id', 'neoforge-1.21.1');
    document.body.appendChild(card);

    const inspectedSample: InspectedDropFile[] = [
      {
        path: 'C:\\mods\\sodium.jar',
        filename: 'sodium.jar',
        size: 1024,
        isDir: false,
        kind: 'Mod',
        detectedLoader: 'fabric',
        displayName: 'Sodium',
      },
    ];
    mockInvoke.mockResolvedValue(inspectedSample);

    const originalElementFromPoint = document.elementFromPoint;
    document.elementFromPoint = vi.fn().mockReturnValue(card);

    try {
      const {
        isDragging,
        hoveredInstanceId,
        draggedFiles,
        initFileDropListener,
        handleDragDropPayload,
      } = useFileDrop();

      const droppedCalls: Array<{ files: InspectedDropFile[]; target: string | null }> = [];
      await initFileDropListener(async (files, target) => {
        droppedCalls.push({ files, target });
      });

      await handleDragDropPayload({
        type: 'enter',
        paths: ['C:\\mods\\sodium.jar'],
        position: { x: 10, y: 20 },
      } as never);

      expect(isDragging.value).toBe(true);
      expect(hoveredInstanceId.value).toBe('neoforge-1.21.1');
      expect(draggedFiles.value).toEqual(inspectedSample);
      expect(mockSetAlwaysOnTop).toHaveBeenCalledWith(true);

      await handleDragDropPayload({
        type: 'drop',
        paths: ['C:\\mods\\sodium.jar'],
        position: { x: 10, y: 20 },
      } as never);

      expect(isDragging.value).toBe(false);
      expect(hoveredInstanceId.value).toBeNull();
      expect(mockSetAlwaysOnTop).toHaveBeenCalledWith(false);
      expect(mockSetFocus).toHaveBeenCalled();
      expect(droppedCalls).toEqual([
        {
          files: inspectedSample,
          target: 'neoforge-1.21.1',
        },
      ]);
    } finally {
      document.elementFromPoint = originalElementFromPoint;
    }
  });

  it('dispatches INSTANCE_ASSETS_UPDATED_EVENT custom event', () => {
    const received: InstanceAssetsUpdatedDetail[] = [];
    const listener = (e: Event) => {
      received.push((e as CustomEvent<InstanceAssetsUpdatedDetail>).detail);
    };
    window.addEventListener(INSTANCE_ASSETS_UPDATED_EVENT, listener);
    try {
      emitInstanceAssetsUpdated('fabric-1.20.1', ['Mod', 'ShaderPack']);
      expect(received).toEqual([
        {
          instanceId: 'fabric-1.20.1',
          kinds: ['Mod', 'ShaderPack'],
        },
      ]);
    } finally {
      window.removeEventListener(INSTANCE_ASSETS_UPDATED_EVENT, listener);
    }
  });
});
