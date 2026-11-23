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
      imagePath: 'C:/dawnland/.dawnland/backgrounds/bg-abc.png',
      fileSizeBytes: 102400,
      blurPx: 10,
      overlayOpacity: 45,
      fit: 'contain',
      updatedAt: 1700000000,
    };
    vi.mocked(invoke).mockResolvedValueOnce(backendState);

    const store = useBackgroundSettings();
    const res = await store.initBackgroundSettings();

    expect(invoke).toHaveBeenCalledWith('get_launcher_background');
    expect(res).toEqual(backendState);
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/dawnland/.dawnland/backgrounds/bg-abc.png',
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

  it('falls back to default wallpaper on image load error and recovers on new selection', async () => {
    const customState: LauncherBackgroundState = {
      mode: 'custom',
      imageFile: 'bg-broken.png',
      imagePath: 'C:/dawnland/.dawnland/backgrounds/bg-broken.png',
      fileSizeBytes: 2048,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 100,
    };
    vi.mocked(invoke).mockResolvedValueOnce(customState);

    const store = useBackgroundSettings();
    await store.initBackgroundSettings();
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/dawnland/.dawnland/backgrounds/bg-broken.png',
    );

    store.handleBackgroundImageError();
    expect(store.imageLoadFailed.value).toBe(true);
    expect(store.resolvedBackgroundSrc.value).toBe('/minecraft_bg.png');

    const nextState: LauncherBackgroundState = {
      ...customState,
      imageFile: 'bg-good.webp',
      imagePath: 'C:/dawnland/.dawnland/backgrounds/bg-good.webp',
      updatedAt: 200,
    };
    vi.mocked(invoke).mockResolvedValueOnce(nextState);
    await store.selectCustomBackground('D:/Pictures/wall.webp');

    expect(invoke).toHaveBeenCalledWith('set_launcher_background', {
      sourcePath: 'D:/Pictures/wall.webp',
    });
    expect(store.imageLoadFailed.value).toBe(false);
    expect(store.resolvedBackgroundSrc.value).toBe(
      'asset://localhost/C:/dawnland/.dawnland/backgrounds/bg-good.webp',
    );
  });

  it('supports live preview, single-flight style commit, and rollback on failure', async () => {
    const initialState: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 10,
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

  it('computes theme-aware overlay defaults and resets cleanly', async () => {
    expect(getEffectiveOverlayOpacity(null, false)).toBe(30);
    expect(getEffectiveOverlayOpacity(null, true)).toBe(60);
    expect(getEffectiveOverlayOpacity(45, true)).toBe(45);

    const resetResult: LauncherBackgroundState = {
      mode: 'default',
      imageFile: null,
      imagePath: null,
      fileSizeBytes: null,
      blurPx: 4,
      overlayOpacity: null,
      fit: 'cover',
      updatedAt: 500,
    };
    vi.mocked(invoke).mockResolvedValueOnce(resetResult);

    const store = useBackgroundSettings();
    await store.resetBackground(true);
    expect(invoke).toHaveBeenCalledWith('reset_launcher_background', { resetStyle: true });
    expect(store.resolvedBackgroundSrc.value).toBe('/minecraft_bg.png');
  });

  it('ignores stale commitBackgroundStyle responses when superseded by resetBackground', async () => {
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

    await store.resetBackground(true);
    expect(store.backgroundState.value.blurPx).toBe(4);
    expect(store.backgroundState.value.fit).toBe('cover');

    // Now the stale style response resolves; it must NOT clobber reset state or localStorage
    resolveStyle({
      mode: 'custom',
      imageFile: 'bg-stale.png',
      imagePath: 'C:/dawnland/.dawnland/backgrounds/bg-stale.png',
      fileSizeBytes: 512,
      blurPx: 18,
      overlayOpacity: 75,
      fit: 'fill',
      updatedAt: 500,
    });
    await commitPromise;

    expect(store.backgroundState.value.mode).toBe('default');
    expect(store.backgroundState.value.blurPx).toBe(4);
    expect(store.backgroundState.value.overlayOpacity).toBeNull();
    expect(store.backgroundState.value.fit).toBe('cover');
    const cached = JSON.parse(localStorage.getItem('dawnland_bg_cache_v1')!);
    expect(cached).toMatchObject({ mode: 'default', blurPx: 4, fit: 'cover' });
  });
});
