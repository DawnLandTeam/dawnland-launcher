<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { open } from '@tauri-apps/plugin-dialog';
import { useDark } from '@vueuse/core';
import { useI18n } from 'vue-i18n';
import { CircleHelp, Image as ImageIcon, Loader2, RotateCcw, Upload } from '@lucide/vue';
import DButton from '../ui/DButton.vue';
import DToggleGroup, { type ToggleOption } from '../ui/DToggleGroup.vue';
import DTooltip from '../ui/DTooltip.vue';
import { useBackgroundSettings } from '../../composables/useBackgroundSettings';
import { toast } from '../../composables/useToast';
import {
  DEFAULT_BACKGROUND_BLUR_PX,
  MAX_BACKGROUND_BLUR_PX,
  MAX_BACKGROUND_OVERLAY_OPACITY,
  type BackgroundFit,
  type UpdateBackgroundStylePayload,
} from '../../types/background';
import { getErrorMessage } from '../../utils/error';
import { formatBytes } from '../../utils/format';
import { createLogger } from '../../utils/logger';

const logger = createLogger('AppearanceSettings');
const { t } = useI18n();
const isDark = useDark();

const {
  backgroundState,
  isUpdating,
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
} = useBackgroundSettings();

onMounted(() => {
  void initBackgroundSettings();
});

const fitOptions = computed<ToggleOption[]>(() => [
  { label: t('settings.appearance.fitCover'), value: 'cover' },
  { label: t('settings.appearance.fitContain'), value: 'contain' },
  { label: t('settings.appearance.fitFill'), value: 'fill' },
]);

const effectiveOverlayOpacity = computed(() =>
  getEffectiveOverlayOpacity(backgroundState.value.overlayOpacity, isDark.value),
);

const isOverlayAuto = computed(() => backgroundState.value.overlayOpacity === null);

const canReset = computed(
  () =>
    backgroundState.value.mode === 'custom' ||
    backgroundState.value.blurPx !== DEFAULT_BACKGROUND_BLUR_PX ||
    backgroundState.value.overlayOpacity !== null ||
    backgroundState.value.fit !== 'cover',
);

const customImageSummary = computed(() => {
  if (!hasCustomImage.value || !backgroundState.value.imageFile) {
    return t('settings.appearance.sourceDefault');
  }
  const ext = backgroundState.value.imageFile.split('.').pop()?.toUpperCase() || 'IMG';
  const sizeText =
    typeof backgroundState.value.fileSizeBytes === 'number'
      ? ` · ${formatBytes(backgroundState.value.fileSizeBytes, 1)}`
      : '';
  return `${t('settings.appearance.sourceCustom')} (${ext}${sizeText})`;
});

const blurOverlayStyle = computed(() => {
  const blur = backgroundState.value.blurPx;
  const filterValue = blur > 0 ? `blur(${blur}px)` : 'none';
  return {
    backdropFilter: filterValue,
    WebkitBackdropFilter: filterValue,
  };
});

const tintOverlayStyle = computed(() => ({
  opacity: effectiveOverlayOpacity.value / 100,
}));

async function handleSelectImage(): Promise<void> {
  if (isUpdating.value) return;
  try {
    const selected = await open({
      title: t('settings.appearance.dialogTitle'),
      directory: false,
      multiple: false,
      filters: [
        {
          name: t('settings.appearance.imageFilterName'),
          extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
        },
      ],
    });
    if (typeof selected !== 'string' || !selected.trim()) {
      return;
    }
    await selectCustomBackground(selected);
    toast.success(t('settings.appearance.setSuccess'));
  } catch (err) {
    logger.error('Failed to set custom launcher background:', err);
    toast.error(t('settings.appearance.setFailed'), getErrorMessage(err));
  }
}

async function handleResetBackground(): Promise<void> {
  if (isUpdating.value || !canReset.value) return;
  try {
    await resetBackground(true);
    toast.success(t('settings.appearance.resetSuccess'));
  } catch (err) {
    logger.error('Failed to reset launcher background:', err);
    toast.error(t('settings.appearance.resetFailed'), getErrorMessage(err));
  }
}

async function persistStyle(partial: Partial<UpdateBackgroundStylePayload>): Promise<void> {
  try {
    await commitBackgroundStyle(partial);
  } catch (err) {
    logger.error('Failed to persist background style:', err);
    toast.error(t('settings.appearance.styleSaveFailed'), getErrorMessage(err));
  }
}

function isBackgroundFit(value: string | number): value is BackgroundFit {
  return value === 'cover' || value === 'contain' || value === 'fill';
}

function handleFitChange(value: string | number): void {
  if (!isBackgroundFit(value)) return;
  previewBackgroundStyle({ fit: value });
  void persistStyle({ fit: value });
}

function handleBlurInput(event: Event): void {
  const target = event.target as HTMLInputElement;
  previewBackgroundStyle({ blurPx: Number(target.value) });
}

function handleBlurChange(event: Event): void {
  const target = event.target as HTMLInputElement;
  void persistStyle({ blurPx: Number(target.value) });
}

function handleOverlayInput(event: Event): void {
  const target = event.target as HTMLInputElement;
  previewBackgroundStyle({ overlayOpacity: Number(target.value) });
}

function handleOverlayChange(event: Event): void {
  const target = event.target as HTMLInputElement;
  void persistStyle({ overlayOpacity: Number(target.value) });
}

function handleResetAutoOverlay(): void {
  previewBackgroundStyle({ overlayOpacity: null });
  void persistStyle({ overlayOpacity: null });
}
</script>

<template>
  <div class="rounded-lg border border-white/20 bg-white/60 p-5 dark:bg-zinc-900/60 backdrop-blur-md shadow-sm space-y-5">
    <!-- Header & Primary Actions -->
    <div class="flex flex-wrap items-start justify-between gap-4">
      <div class="space-y-1">
        <div class="flex items-center gap-2.5">
          <ImageIcon :size="18" class="text-primary shrink-0" />
          <h2 class="text-base font-semibold flex items-center gap-1.5 text-foreground">
            {{ t('settings.appearance.backgroundTitle') }}
            <DTooltip :content="t('settings.appearance.backgroundDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="15" />
              </span>
            </DTooltip>
          </h2>
        </div>
        <p class="text-xs text-muted-foreground">
          {{ t('settings.appearance.backgroundDesc') }}
        </p>
      </div>

      <div class="flex items-center gap-2 shrink-0">
        <DButton
          data-testid="choose-background-btn"
          variant="primary"
          size="sm"
          :disabled="isUpdating"
          @click="handleSelectImage"
        >
          <Loader2 v-if="isUpdating" class="w-4 h-4 mr-1.5 animate-spin" />
          <Upload v-else class="w-4 h-4 mr-1.5" />
          {{ t('settings.appearance.selectImageBtn') }}
        </DButton>
        <DButton
          data-testid="reset-background-btn"
          variant="outline"
          size="sm"
          :disabled="isUpdating || !canReset"
          @click="handleResetBackground"
        >
          <RotateCcw class="w-4 h-4 mr-1.5" />
          {{ t('settings.appearance.resetDefaultBtn') }}
        </DButton>
      </div>
    </div>

    <!-- Live 16:10 Mini Preview -->
    <div class="space-y-2">
      <div class="flex items-center justify-between text-xs text-muted-foreground">
        <span class="font-medium text-foreground">{{ t('settings.appearance.previewLabel') }}</span>
        <span>{{ t('settings.appearance.formatHint') }}</span>
      </div>

      <div
        data-testid="background-preview-box"
        class="relative w-full max-w-xl aspect-[16/10] rounded-lg overflow-hidden border border-neutral-200/70 dark:border-zinc-700/60 bg-neutral-100 dark:bg-zinc-950 shadow-inner"
      >
        <img
          :src="resolvedBackgroundSrc"
          alt=""
          :class="['w-full h-full select-none', fitObjectClass]"
          draggable="false"
          @error="handleBackgroundImageError"
        />
        <div
          data-testid="preview-blur-layer"
          class="absolute inset-0 pointer-events-none"
          :style="blurOverlayStyle"
        ></div>
        <div
          data-testid="preview-tint-layer"
          class="absolute inset-0 pointer-events-none bg-white dark:bg-black transition-opacity duration-200"
          :style="tintOverlayStyle"
        ></div>

        <!-- Mini Launcher Skeleton for Contrast Inspection -->
        <div class="relative z-10 flex flex-col h-full pointer-events-none select-none">
          <div class="h-5 px-2.5 flex items-center justify-between bg-white/15 dark:bg-black/25 border-b border-white/15">
            <span class="text-[10px] font-medium text-neutral-800 dark:text-zinc-200">
              {{ t('settings.appearance.previewSampleTitle') }}
            </span>
            <div class="flex items-center gap-1">
              <span class="w-1.5 h-1.5 rounded-full bg-neutral-500/60 dark:bg-zinc-400/60"></span>
              <span class="w-1.5 h-1.5 rounded-full bg-neutral-500/60 dark:bg-zinc-400/60"></span>
            </div>
          </div>
          <div class="flex flex-1 min-h-0">
            <div class="w-9 border-r border-white/15 bg-white/10 dark:bg-black/20 flex flex-col items-center py-2 gap-1.5">
              <span class="w-4 h-4 rounded bg-black/15 dark:bg-white/15"></span>
              <span class="w-4 h-4 rounded bg-black/5 dark:bg-white/5"></span>
              <span class="w-4 h-4 rounded bg-black/5 dark:bg-white/5"></span>
            </div>
            <div class="flex-1 p-3 flex flex-col justify-between">
              <div class="rounded-md border border-white/25 bg-white/60 dark:bg-zinc-900/60 backdrop-blur-sm p-2.5 shadow-sm max-w-[72%]">
                <p class="text-xs font-semibold text-neutral-900 dark:text-zinc-100">
                  {{ t('settings.appearance.previewSampleSubtitle') }}
                </p>
                <div class="mt-1.5 h-1.5 w-28 rounded bg-neutral-900/15 dark:bg-white/15"></div>
                <div class="mt-1 h-1.5 w-20 rounded bg-neutral-900/10 dark:bg-white/10"></div>
              </div>
              <div class="flex items-center justify-between">
                <span
                  data-testid="background-source-badge"
                  class="inline-flex items-center px-2 py-0.5 rounded-md text-[11px] font-medium bg-white/75 dark:bg-zinc-900/75 text-foreground border border-white/30 dark:border-zinc-700/60 shadow-sm"
                >
                  {{ customImageSummary }}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Visual Fine-Tuning Controls -->
    <div class="pt-4 border-t border-black/5 dark:border-white/5 space-y-4">
      <!-- Fill Mode -->
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div class="space-y-0.5">
          <span class="text-sm font-medium flex items-center gap-1.5 text-foreground">
            {{ t('settings.appearance.fitModeLabel') }}
            <DTooltip :content="t('settings.appearance.fitModeDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="14" />
              </span>
            </DTooltip>
          </span>
        </div>
        <DToggleGroup
          :model-value="backgroundState.fit"
          :options="fitOptions"
          @update:model-value="handleFitChange"
        />
      </div>

      <!-- Blur Slider -->
      <div class="space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm font-medium flex items-center gap-1.5 text-foreground">
            {{ t('settings.appearance.blurLabel') }}
            <DTooltip :content="t('settings.appearance.blurDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="14" />
              </span>
            </DTooltip>
          </span>
          <span data-testid="background-blur-value" class="text-sm font-mono text-primary font-semibold">
            {{ backgroundState.blurPx }} px
          </span>
        </div>
        <input
          data-testid="background-blur-slider"
          type="range"
          min="0"
          :max="MAX_BACKGROUND_BLUR_PX"
          step="1"
          :value="backgroundState.blurPx"
          @input="handleBlurInput"
          @change="handleBlurChange"
          class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-primary"
        />
        <div class="flex justify-between text-xs text-muted-foreground">
          <span>0 px</span>
          <span>{{ MAX_BACKGROUND_BLUR_PX }} px</span>
        </div>
      </div>

      <!-- Overlay Opacity Slider -->
      <div class="space-y-1.5">
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm font-medium flex items-center gap-1.5 text-foreground">
            {{ t('settings.appearance.overlayLabel') }}
            <DTooltip :content="t('settings.appearance.overlayDesc')">
              <span class="inline-flex p-0.5 rounded-full text-muted-foreground/60 hover:text-foreground hover:bg-neutral-200/50 dark:hover:bg-zinc-800/50 cursor-help transition-colors">
                <CircleHelp :size="14" />
              </span>
            </DTooltip>
          </span>
          <div class="flex items-center gap-2">
            <span
              v-if="isOverlayAuto"
              data-testid="overlay-auto-badge"
              class="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-medium bg-emerald-500/10 text-emerald-700 dark:text-emerald-300 border border-emerald-500/20"
            >
              {{ t('settings.appearance.overlayAutoBadge') }}
            </span>
            <DButton
              v-else
              data-testid="reset-auto-overlay-btn"
              variant="ghost"
              size="sm"
              class="h-6 px-2 text-xs"
              @click="handleResetAutoOverlay"
            >
              {{ t('settings.appearance.resetAutoOverlayBtn') }}
            </DButton>
            <span data-testid="background-overlay-value" class="text-sm font-mono text-primary font-semibold">
              {{ effectiveOverlayOpacity }}%
            </span>
          </div>
        </div>
        <input
          data-testid="background-overlay-slider"
          type="range"
          min="0"
          :max="MAX_BACKGROUND_OVERLAY_OPACITY"
          step="5"
          :value="effectiveOverlayOpacity"
          @input="handleOverlayInput"
          @change="handleOverlayChange"
          class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer dark:bg-zinc-800 accent-primary"
        />
        <div class="flex justify-between text-xs text-muted-foreground">
          <span>0%</span>
          <span>{{ MAX_BACKGROUND_OVERLAY_OPACITY }}%</span>
        </div>
      </div>
    </div>
  </div>
</template>
