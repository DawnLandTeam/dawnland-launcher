import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  __resetBackgroundStoreForTesting,
  getEffectiveOverlayOpacity,
  useBackgroundSettings,
} from './useBackgroundSettings';
import type { LauncherBackgroundState } from '../types/background';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${path}`),
}));

describe('useBackgroundSettings', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    __resetBackgroundStoreForTesting();
  });

  it('initializes from backend and syncs localStorage cache', async () => {
    const backendState: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-abc.png',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-abc.png',
      fileSizeBytes: 102400,
      blurPx: 10,
      overlayOpacity: 45,
      fit: 'contain',
      updatedAt: 1700000000,
      revision: 3,
      resetEpoch: 1,
    };
    vi.mocked(invoke).mockResolvedValueOnce(backendState);

    const store = useBackgroundSettings();
    const res = await store.initBackgroundSettings();

    expect(invoke).toHaveBeenCalledWith('get_launcher_background');
    expect(res).toEqual(backendState);
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/AppData/com.dawnland.launcher/backgrounds/bg-abc.png',
    );
    expect(store.fitObjectClass.value).toBe('object-contain');

    const cachedRaw = localStorage.getItem('dawnland_bg_cache_v1');
    expect(cachedRaw).toBeTruthy();
    expect(JSON.parse(cachedRaw!)).toMatchObject({
      mode: 'custom',
      imageFile: 'bg-abc.png',
      blurPx: 10,
    });
  });

  it('gracefully retains defaults when backend returns undefined', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    const store = useBackgroundSettings();
    const res = await store.initBackgroundSettings();

    expect(res.mode).toBe('default');
    expect(store.resolvedBackgroundSrc.value).toBe('/minecraft_bg.png');
    expect(store.fitObjectClass.value).toBe('object-cover');
  });

  it('falls back to default wallpaper on image load error and recovers on native pick', async () => {
    const customState: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-broken.png',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-broken.png',
      fileSizeBytes: 2048,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 100,
      revision: 1,
      resetEpoch: 0,
    };
    vi.mocked(invoke).mockResolvedValueOnce(customState);

    const store = useBackgroundSettings();
    await store.initBackgroundSettings();
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/AppData/com.dawnland.launcher/backgrounds/bg-broken.png',
    );

    store.handleBackgroundImageError();
    expect(store.imageLoadFailed.value).toBe(true);
    expect(store.resolvedBackgroundSrc.value).toBe('/minecraft_bg.png');

    // Cancelled dialog returns null without replacing state
    vi.mocked(invoke).mockResolvedValueOnce(null);
    const cancelled = await store.pickCustomBackground({
      dialogTitle: 'Select Background',
      filterName: 'Images',
    });
    expect(cancelled).toBeNull();
    expect(store.imageLoadFailed.value).toBe(true);

    const nextState: LauncherBackgroundState = {
      ...customState,
      imageFile: 'bg-good.webp',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-good.webp',
      updatedAt: 200,
      revision: 2,
    };
    vi.mocked(invoke).mockResolvedValueOnce(nextState);
    await store.pickCustomBackground({
      dialogTitle: 'Select Background',
      filterName: 'Images',
    });

    expect(invoke).toHaveBeenCalledWith('pick_and_set_launcher_background', {
      dialogTitle: 'Select Background',
      filterName: 'Images',
    });
    expect(store.imageLoadFailed.value).toBe(false);
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/AppData/com.dawnland.launcher/backgrounds/bg-good.webp',
    );
  });

  it('supports live preview and rolls back on style commit failure when queue is empty', async () => {
    const initialState: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 10,
      revision: 0,
      resetEpoch: 0,
    };
    vi.mocked(invoke).mockResolvedValueOnce(initialState);

    const store = useBackgroundSettings();
    await store.initBackgroundSettings();

    // Local preview clamps values without calling invoke
    store.previewBackgroundStyle({ blurPx: 99, overlayOpacity: 95, fit: 'fill' });
    expect(store.backgroundState.value.blurPx).toBe(24);
    expect(store.backgroundState.value.overlayOpacity).toBe(85);
    expect(store.backgroundState.value.fit).toBe('fill');
    expect(invoke).toHaveBeenCalledTimes(1);

    // Commit failure rolls back to last confirmed state
    vi.mocked(invoke).mockRejectedValueOnce(new Error('disk full'));
    await expect(store.commitBackgroundStyle({ blurPx: 16, fit: 'fill' })).rejects.toThrow(
      'disk full',
    );
    expect(store.backgroundState.value.blurPx).toBe(4);
    expect(store.backgroundState.value.overlayOpacity).toBeNull();
    expect(store.backgroundState.value.fit).toBe('cover');
  });

  it('preserves and sends newer queued style change when an in-flight style request fails', async () => {
    let rejectFirst!: (err: Error) => void;
    const secondResult: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 20,
      overlayOpacity: 50,
      fit: 'contain',
      updatedAt: 200,
      revision: 2,
      resetEpoch: 0,
    };

    let styleCalls = 0;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'update_launcher_background_style') {
        styleCalls += 1;
        if (styleCalls === 1) {
          return new Promise<LauncherBackgroundState>((_resolve, reject) => {
            rejectFirst = reject;
          });
        }
        return secondResult;
      }
      return undefined;
    });

    const store = useBackgroundSettings();
    const p1 = store.commitBackgroundStyle({ blurPx: 8, overlayOpacity: 20, fit: 'cover' });
    const p2 = store.commitBackgroundStyle({ blurPx: 20, overlayOpacity: 50, fit: 'contain' });

    rejectFirst(new Error('transient io failure'));

    await expect(p1).resolves.toEqual(secondResult);
    await expect(p2).resolves.toEqual(secondResult);

    expect(styleCalls).toBe(2);
    expect(invoke).toHaveBeenLastCalledWith('update_launcher_background_style', {
      blurPx: 20,
      overlayOpacity: 50,
      fit: 'contain',
      expectedResetEpoch: 0,
    });
    expect(store.backgroundState.value.blurPx).toBe(20);
    expect(store.backgroundState.value.overlayOpacity).toBe(50);
    expect(store.backgroundState.value.fit).toBe('contain');
  });

  it('applies imported background even when a concurrent style update commits first', async () => {
    let resolvePick!: (val: LauncherBackgroundState) => void;
    const styleState: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 14,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 100,
      revision: 1,
      resetEpoch: 0,
    };
    const pickedState: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-new.png',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-new.png',
      fileSizeBytes: 4096,
      blurPx: 14,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 200,
      revision: 2,
      resetEpoch: 0,
    };

    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'pick_and_set_launcher_background') {
        return new Promise<LauncherBackgroundState>((resolve) => {
          resolvePick = resolve;
        });
      }
      if (cmd === 'update_launcher_background_style') {
        return Promise.resolve(styleState);
      }
      return Promise.resolve(undefined);
    });

    const store = useBackgroundSettings();
    const pickPromise = store.pickCustomBackground();
    await store.commitBackgroundStyle({ blurPx: 14 });
    expect(store.backgroundState.value.blurPx).toBe(14);
    expect(store.backgroundState.value.mode).toBe('default');

    resolvePick(pickedState);
    await pickPromise;

    expect(store.backgroundState.value.mode).toBe('custom');
    expect(store.backgroundState.value.imageFile).toBe('bg-new.png');
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/AppData/com.dawnland.launcher/backgrounds/bg-new.png',
    );
  });

  it('computes theme-aware overlay defaults, sends expectedResetEpoch, and ignores lower-revision responses', async () => {
    expect(getEffectiveOverlayOpacity(null, false)).toBe(30);
    expect(getEffectiveOverlayOpacity(null, true)).toBe(60);
    expect(getEffectiveOverlayOpacity(45, true)).toBe(45);

    let resolveStyle!: (value: LauncherBackgroundState) => void;
    const resetResult: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 900,
      revision: 5,
      resetEpoch: 2,
    };

    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === 'update_launcher_background_style') {
        return new Promise<LauncherBackgroundState>((resolve) => {
          resolveStyle = resolve;
        });
      }
      if (cmd === 'reset_launcher_background') {
        return Promise.resolve(resetResult);
      }
      return Promise.resolve(undefined);
    });

    const store = useBackgroundSettings();
    const commitPromise = store.commitBackgroundStyle({ blurPx: 18, overlayOpacity: 75, fit: 'fill' });
    expect(invoke).toHaveBeenCalledWith('update_launcher_background_style', {
      blurPx: 18,
      overlayOpacity: 75,
      fit: 'fill',
      expectedResetEpoch: 0,
    });

    await store.resetBackground(true);
    expect(invoke).toHaveBeenCalledWith('reset_launcher_background', { resetStyle: true });
    expect(store.backgroundState.value.blurPx).toBe(4);
    expect(store.backgroundState.value.fit).toBe('cover');

    // Stale lower-revision style response must not overwrite reset state or localStorage
    resolveStyle({
      mode: 'custom',
      imageFile: 'bg-stale.png',
      imagePath: 'C:/AppData/com.dawnland.launcher/backgrounds/bg-stale.png',
      fileSizeBytes: 512,
      blurPx: 18,
      overlayOpacity: 75,
      fit: 'fill',
      updatedAt: 500,
      revision: 4,
      resetEpoch: 1,
    });
    await commitPromise;

    expect(store.backgroundState.value.mode).toBe('default');
    expect(store.backgroundState.value.blurPx).toBe(4);
    expect(store.backgroundState.value.overlayOpacity).toBeNull();
    expect(store.backgroundState.value.fit).toBe('cover');
    const cached = JSON.parse(localStorage.getItem('dawnland_bg_cache_v1')!);
    expect(cached).toMatchObject({ mode: 'default', blurPx: 4, fit: 'cover', resetEpoch: 2 });
  });
});
