import { computed, ref } from 'vue';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { createLogger } from '../utils/logger';
import {
  DEFAULT_BACKGROUND_BLUR_PX,
  DEFAULT_BACKGROUND_SRC,
  DEFAULT_DARK_OVERLAY_OPACITY,
  DEFAULT_LIGHT_OVERLAY_OPACITY,
  MAX_BACKGROUND_BLUR_PX,
  MAX_BACKGROUND_OVERLAY_OPACITY,
  type BackgroundFit,
  type BackgroundMode,
  type LauncherBackgroundState,
  type UpdateBackgroundStylePayload,
} from '../types/background';

const logger = createLogger('useBackgroundSettings');

const BACKGROUND_CACHE_KEY = 'dawnland_bg_cache_v1';

export function createDefaultBackgroundState(): LauncherBackgroundState {
  return {
    mode: 'default',
    imageFile: null,
    imagePath: null,
    fileSizeBytes: null,
    blurPx: DEFAULT_BACKGROUND_BLUR_PX,
    overlayOpacity: null,
    fit: 'cover',
    updatedAt: 0,
  };
}

function clampBlur(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return DEFAULT_BACKGROUND_BLUR_PX;
  }
  return Math.max(0, Math.min(MAX_BACKGROUND_BLUR_PX, Math.round(value)));
}

function clampOverlay(value: unknown): number | null {
  if (value === null || value === undefined) {
    return null;
  }
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }
  return Math.max(0, Math.min(MAX_BACKGROUND_OVERLAY_OPACITY, Math.round(value)));
}

function isValidMode(value: unknown): value is BackgroundMode {
  return value === 'default' || value === 'custom';
}

function isValidFit(value: unknown): value is BackgroundFit {
  return value === 'cover' || value === 'contain' || value === 'fill';
}

function isBackgroundStateObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function normalizeBackgroundState(raw: unknown): LauncherBackgroundState | null {
  if (!isBackgroundStateObject(raw)) {
    return null;
  }
  const mode: BackgroundMode = isValidMode(raw.mode) ? raw.mode : 'default';
  const fit: BackgroundFit = isValidFit(raw.fit) ? raw.fit : 'cover';
  const imageFile = typeof raw.imageFile === 'string' && raw.imageFile.trim() ? raw.imageFile.trim() : null;
  const imagePath = typeof raw.imagePath === 'string' && raw.imagePath.trim() ? raw.imagePath.trim() : null;
  const fileSizeBytes =
    typeof raw.fileSizeBytes === 'number' && Number.isFinite(raw.fileSizeBytes) && raw.fileSizeBytes >= 0
      ? raw.fileSizeBytes
      : null;
  const updatedAt =
    typeof raw.updatedAt === 'number' && Number.isFinite(raw.updatedAt) && raw.updatedAt >= 0
      ? raw.updatedAt
      : 0;

  return {
    mode,
    imageFile,
    imagePath: mode === 'custom' ? imagePath : null,
    fileSizeBytes: mode === 'custom' ? fileSizeBytes : null,
    blurPx: clampBlur(raw.blurPx),
    overlayOpacity: clampOverlay(raw.overlayOpacity),
    fit,
    updatedAt,
  };
}

function readCachedBackgroundState(): LauncherBackgroundState {
  if (typeof localStorage === 'undefined') {
    return createDefaultBackgroundState();
  }
  try {
    const raw = localStorage.getItem(BACKGROUND_CACHE_KEY);
    if (!raw) {
      return createDefaultBackgroundState();
    }
    const parsed: unknown = JSON.parse(raw);
    return normalizeBackgroundState(parsed) ?? createDefaultBackgroundState();
  } catch (err) {
    logger.warn('Failed to read cached background state from localStorage:', err);
    return createDefaultBackgroundState();
  }
}

function writeCachedBackgroundState(state: LauncherBackgroundState): void {
  if (typeof localStorage === 'undefined') {
    return;
  }
  try {
    localStorage.setItem(BACKGROUND_CACHE_KEY, JSON.stringify(state));
  } catch (err) {
    logger.warn('Failed to persist background state cache:', err);
  }
}

function toAssetUrl(path: string): string {
  if (typeof convertFileSrc === 'function') {
    return convertFileSrc(path);
  }
  return path;
}

const initialCached = readCachedBackgroundState();
const backgroundState = ref<LauncherBackgroundState>({ ...initialCached });
let lastConfirmedState: LauncherBackgroundState = { ...initialCached };

const isLoading = ref(false);
const isUpdating = ref(false);
const imageLoadFailed = ref(false);
const isInitialized = ref(false);

let mutationSeq = 0;
let activeInitPromise: Promise<LauncherBackgroundState> | null = null;
let activeStyleCommitPromise: Promise<LauncherBackgroundState | null> | null = null;
let pendingStylePayload: UpdateBackgroundStylePayload | null = null;

function applyConfirmedState(next: LauncherBackgroundState): void {
  const normalized = normalizeBackgroundState(next) ?? createDefaultBackgroundState();
  lastConfirmedState = { ...normalized };
  backgroundState.value = { ...normalized };
  imageLoadFailed.value = false;
  writeCachedBackgroundState(normalized);
}

const hasCustomImage = computed(
  () =>
    backgroundState.value.mode === 'custom' &&
    Boolean(backgroundState.value.imagePath) &&
    !imageLoadFailed.value,
);

const resolvedBackgroundSrc = computed(() => {
  if (hasCustomImage.value && backgroundState.value.imagePath) {
    return toAssetUrl(backgroundState.value.imagePath);
  }
  return DEFAULT_BACKGROUND_SRC;
});

const fitObjectClass = computed(() => {
  switch (backgroundState.value.fit) {
    case 'contain':
      return 'object-contain';
    case 'fill':
      return 'object-fill';
    case 'cover':
    default:
      return 'object-cover';
  }
});

export function getEffectiveOverlayOpacity(
  overlayOpacity: number | null | undefined,
  isDark: boolean,
): number {
  if (typeof overlayOpacity === 'number' && Number.isFinite(overlayOpacity)) {
    return clampOverlay(overlayOpacity) ?? (isDark ? DEFAULT_DARK_OVERLAY_OPACITY : DEFAULT_LIGHT_OVERLAY_OPACITY);
  }
  return isDark ? DEFAULT_DARK_OVERLAY_OPACITY : DEFAULT_LIGHT_OVERLAY_OPACITY;
}

export async function initBackgroundSettings(force = false): Promise<LauncherBackgroundState> {
  if (isInitialized.value && !force) {
    return backgroundState.value;
  }
  if (activeInitPromise && !force) {
    return activeInitPromise;
  }

  const seqAtStart = mutationSeq;
  isLoading.value = true;

  const task = (async () => {
    try {
      const result = await invoke<LauncherBackgroundState | undefined>('get_launcher_background');
      const normalized = normalizeBackgroundState(result);
      if (normalized && seqAtStart === mutationSeq) {
        applyConfirmedState(normalized);
      }
      isInitialized.value = true;
      return backgroundState.value;
    } catch (err) {
      logger.warn('Failed to load launcher background settings:', err);
      return backgroundState.value;
    } finally {
      isLoading.value = false;
      activeInitPromise = null;
    }
  })();

  activeInitPromise = task;
  return task;
}

export async function selectCustomBackground(sourcePath: string): Promise<LauncherBackgroundState> {
  const trimmed = sourcePath.trim();
  isUpdating.value = true;
  const currentSeq = ++mutationSeq;
  try {
    const result = await invoke<LauncherBackgroundState>('set_launcher_background', {
      sourcePath: trimmed,
    });
    const normalized = normalizeBackgroundState(result);
    if (!normalized) {
      throw new Error('Invalid background state returned from backend');
    }
    if (currentSeq === mutationSeq) {
      applyConfirmedState(normalized);
      isInitialized.value = true;
    }
    return backgroundState.value;
  } finally {
    isUpdating.value = false;
  }
}

export function previewBackgroundStyle(partial: Partial<UpdateBackgroundStylePayload>): void {
  backgroundState.value = {
    ...backgroundState.value,
    blurPx: partial.blurPx !== undefined ? clampBlur(partial.blurPx) : backgroundState.value.blurPx,
    overlayOpacity:
      partial.overlayOpacity !== undefined
        ? clampOverlay(partial.overlayOpacity)
        : backgroundState.value.overlayOpacity,
    fit: partial.fit !== undefined && isValidFit(partial.fit) ? partial.fit : backgroundState.value.fit,
  };
}

export async function commitBackgroundStyle(
  partial?: Partial<UpdateBackgroundStylePayload>,
): Promise<LauncherBackgroundState | null> {
  if (partial) {
    previewBackgroundStyle(partial);
  }

  pendingStylePayload = {
    blurPx: backgroundState.value.blurPx,
    overlayOpacity: backgroundState.value.overlayOpacity,
    fit: backgroundState.value.fit,
  };

  if (activeStyleCommitPromise) {
    return activeStyleCommitPromise;
  }

  const runCommitQueue = (async (): Promise<LauncherBackgroundState | null> => {
    let latestState: LauncherBackgroundState | null = null;
    while (pendingStylePayload) {
      const payload = pendingStylePayload;
      pendingStylePayload = null;
      const seqAtStart = ++mutationSeq;

      try {
        const result = await invoke<LauncherBackgroundState>('update_launcher_background_style', {
          blurPx: payload.blurPx,
          overlayOpacity: payload.overlayOpacity,
          fit: payload.fit,
        });
        const normalized = normalizeBackgroundState(result);
        if (normalized && seqAtStart === mutationSeq) {
          if (!pendingStylePayload) {
            applyConfirmedState(normalized);
            isInitialized.value = true;
            latestState = backgroundState.value;
          } else {
            lastConfirmedState = { ...normalized };
            writeCachedBackgroundState(normalized);
            latestState = normalized;
          }
        }
      } catch (err) {
        pendingStylePayload = null;
        if (seqAtStart === mutationSeq) {
          backgroundState.value = {
            ...backgroundState.value,
            blurPx: lastConfirmedState.blurPx,
            overlayOpacity: lastConfirmedState.overlayOpacity,
            fit: lastConfirmedState.fit,
          };
        }
        throw err;
      }
    }
    return latestState;
  })();

  activeStyleCommitPromise = runCommitQueue.finally(() => {
    activeStyleCommitPromise = null;
  });

  return activeStyleCommitPromise;
}

export async function resetBackground(resetStyle = true): Promise<LauncherBackgroundState> {
  isUpdating.value = true;
  pendingStylePayload = null;
  const currentSeq = ++mutationSeq;
  try {
    const result = await invoke<LauncherBackgroundState>('reset_launcher_background', {
      resetStyle,
    });
    const normalized = normalizeBackgroundState(result);
    if (!normalized) {
      throw new Error('Invalid background state returned from backend');
    }
    if (currentSeq === mutationSeq) {
      applyConfirmedState(normalized);
      isInitialized.value = true;
    }
    return backgroundState.value;
  } finally {
    isUpdating.value = false;
  }
}

export function handleBackgroundImageError(): void {
  if (backgroundState.value.mode === 'custom' && backgroundState.value.imagePath) {
    logger.warn('Failed to load custom background image, falling back to default wallpaper.');
    imageLoadFailed.value = true;
  }
}

export function __resetBackgroundStoreForTesting(): void {
  const fresh = createDefaultBackgroundState();
  backgroundState.value = { ...fresh };
  lastConfirmedState = { ...fresh };
  isLoading.value = false;
  isUpdating.value = false;
  imageLoadFailed.value = false;
  isInitialized.value = false;
  mutationSeq = 0;
  activeInitPromise = null;
  activeStyleCommitPromise = null;
  pendingStylePayload = null;
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.removeItem(BACKGROUND_CACHE_KEY);
    } catch {
      // Ignore storage cleanup errors in restricted environments
    }
  }
}

export function useBackgroundSettings() {
  return {
    backgroundState,
    isLoading,
    isUpdating,
    imageLoadFailed,
    isInitialized,
    hasCustomImage,
    resolvedBackgroundSrc,
    fitObjectClass,
    getEffectiveOverlayOpacity,
    initBackgroundSettings,
    selectCustomBackground,
    previewBackgroundStyle,
    commitBackgroundStyle,
    resetBackground,
    handleBackgroundImageError,
  };
}
