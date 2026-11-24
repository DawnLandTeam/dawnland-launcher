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
    revision: 0,
    resetEpoch: 0,
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

function parseNonNegativeInt(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    return 0;
  }
  return Math.floor(value);
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
  const imageFile = typeof raw.imageFile === 'string' && raw.imageFile.trim() ? raw.imageFile : null;
  const imagePath = typeof raw.imagePath === 'string' && raw.imagePath.trim() ? raw.imagePath : null;
  const fileSizeBytes =
    typeof raw.fileSizeBytes === 'number' && Number.isFinite(raw.fileSizeBytes) && raw.fileSizeBytes >= 0
      ? raw.fileSizeBytes
      : null;

  return {
    mode,
    imageFile,
    imagePath: mode === 'custom' ? imagePath : null,
    fileSizeBytes: mode === 'custom' ? fileSizeBytes : null,
    blurPx: clampBlur(raw.blurPx),
    overlayOpacity: clampOverlay(raw.overlayOpacity),
    fit,
    updatedAt: parseNonNegativeInt(raw.updatedAt),
    revision: parseNonNegativeInt(raw.revision),
    resetEpoch: parseNonNegativeInt(raw.resetEpoch),
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
    const normalized = normalizeBackgroundState(parsed);
    if (!normalized) {
      return createDefaultBackgroundState();
    }
    // Never trust cached coordination counters over the live backend session.
    return {
      ...normalized,
      revision: 0,
      resetEpoch: 0,
    };
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

let lastAppliedRevision = 0;
let currentResetEpoch = 0;
let activeInitPromise: Promise<LauncherBackgroundState> | null = null;
let activeStyleCommitPromise: Promise<LauncherBackgroundState | null> | null = null;
let pendingStylePayload: UpdateBackgroundStylePayload | null = null;

function applyAuthoritativeState(
  next: LauncherBackgroundState,
  preserveStylePreview = false,
): void {
  if (next.revision < lastAppliedRevision) {
    return;
  }

  const prevMode = backgroundState.value.mode;
  const prevPath = backgroundState.value.imagePath;

  lastAppliedRevision = next.revision;
  currentResetEpoch = next.resetEpoch;
  lastConfirmedState = { ...next };

  if (next.mode !== prevMode || next.imagePath !== prevPath) {
    imageLoadFailed.value = false;
  }

  if (preserveStylePreview) {
    backgroundState.value = {
      ...next,
      blurPx: backgroundState.value.blurPx,
      overlayOpacity: backgroundState.value.overlayOpacity,
      fit: backgroundState.value.fit,
    };
  } else {
    backgroundState.value = { ...next };
  }

  writeCachedBackgroundState(next);
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
    return (
      clampOverlay(overlayOpacity) ??
      (isDark ? DEFAULT_DARK_OVERLAY_OPACITY : DEFAULT_LIGHT_OVERLAY_OPACITY)
    );
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

  const revAtStart = lastAppliedRevision;
  const epochAtStart = currentResetEpoch;
  isLoading.value = true;

  const task = (async () => {
    try {
      const result = await invoke<LauncherBackgroundState | undefined>('get_launcher_background');
      const normalized = normalizeBackgroundState(result);
      if (normalized) {
        const shouldApply =
          normalized.revision > lastAppliedRevision ||
          (lastAppliedRevision === revAtStart && currentResetEpoch === epochAtStart);
        if (shouldApply) {
          applyAuthoritativeState(normalized, pendingStylePayload !== null);
        }
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

export interface PickBackgroundOptions {
  dialogTitle?: string;
  filterName?: string;
}

export async function pickCustomBackground(
  options?: PickBackgroundOptions,
): Promise<LauncherBackgroundState | null> {
  isUpdating.value = true;
  try {
    const result = await invoke<LauncherBackgroundState | null>(
      'pick_and_set_launcher_background',
      {
        dialogTitle: options?.dialogTitle ?? null,
        filterName: options?.filterName ?? null,
      },
    );
    if (result === null || result === undefined) {
      return null;
    }
    const normalized = normalizeBackgroundState(result);
    if (!normalized) {
      throw new Error('Invalid background state returned from backend');
    }
    imageLoadFailed.value = false;
    applyAuthoritativeState(normalized, pendingStylePayload !== null);
    isInitialized.value = true;
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
      const expectedEpoch = currentResetEpoch;

      try {
        const result = await invoke<LauncherBackgroundState>('update_launcher_background_style', {
          blurPx: payload.blurPx,
          overlayOpacity: payload.overlayOpacity,
          fit: payload.fit,
          expectedResetEpoch: expectedEpoch,
        });
        const normalized = normalizeBackgroundState(result);
        if (normalized) {
          applyAuthoritativeState(normalized, pendingStylePayload !== null);
          isInitialized.value = true;
          latestState = backgroundState.value;
        }
      } catch (err) {
        if (pendingStylePayload) {
          // A newer adjustment was queued while this request was in flight; continue so it is sent.
          logger.warn('Background style update failed; retrying with newer queued values:', err);
          continue;
        }
        if (expectedEpoch === currentResetEpoch) {
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
  try {
    const result = await invoke<LauncherBackgroundState>('reset_launcher_background', {
      resetStyle,
    });
    const normalized = normalizeBackgroundState(result);
    if (!normalized) {
      throw new Error('Invalid background state returned from backend');
    }
    imageLoadFailed.value = false;
    applyAuthoritativeState(normalized, false);
    isInitialized.value = true;
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
  lastAppliedRevision = 0;
  currentResetEpoch = 0;
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
    pickCustomBackground,
    previewBackgroundStyle,
    commitBackgroundStyle,
    resetBackground,
    handleBackgroundImageError,
  };
}
