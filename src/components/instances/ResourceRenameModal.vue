<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { ArrowRight, Check, FolderSync, Loader2, RotateCcw } from '@lucide/vue';
import { DialogContent, DialogDescription, DialogTitle } from '../ui/dialog';
import DButton from '../ui/DButton.vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';
import type {
  AssetCategory,
  RenameApplyResult,
  RenamePreviewItem,
} from '../../types/resourceNaming';

const props = defineProps<{
  open: boolean;
  instanceId: string;
  categories?: AssetCategory[];
}>();

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void;
  (e: 'applied', result: RenameApplyResult): void;
}>();

const isRestoreMode = ref(false);
const isLoadingPreview = ref(false);
const isApplying = ref(false);
const previewItems = ref<RenamePreviewItem[]>([]);
const selectedPaths = ref<Set<string>>(new Set());
const showUnchanged = ref(false);

const changeableItems = computed(() => previewItems.value.filter((item) => item.willChange));
const visibleItems = computed(() =>
  showUnchanged.value ? previewItems.value : changeableItems.value,
);
const selectedChangeableCount = computed(
  () => changeableItems.value.filter((item) => selectedPaths.value.has(item.currentRelPath)).length,
);

async function loadPreview() {
  if (!props.open || !props.instanceId) return;
  isLoadingPreview.value = true;
  try {
    const items = await invoke<RenamePreviewItem[]>('preview_resource_renames', {
      instanceId: props.instanceId,
      restoreOriginal: isRestoreMode.value,
      categories: props.categories ?? null,
    });
    previewItems.value = items;
    const nextSelected = new Set<string>();
    for (const item of items) {
      if (item.willChange) {
        nextSelected.add(item.currentRelPath);
      }
    }
    selectedPaths.value = nextSelected;
  } catch (err) {
    toast.error(getErrorMessage(err));
  } finally {
    isLoadingPreview.value = false;
  }
}

watch(
  () => [props.open, props.instanceId, isRestoreMode.value],
  ([openVal]) => {
    if (openVal) {
      loadPreview();
    }
  },
);

function toggleSelection(relPath: string) {
  const next = new Set(selectedPaths.value);
  if (next.has(relPath)) {
    next.delete(relPath);
  } else {
    next.add(relPath);
  }
  selectedPaths.value = next;
}

function toggleSelectAll() {
  if (selectedChangeableCount.value === changeableItems.value.length) {
    selectedPaths.value = new Set();
  } else {
    selectedPaths.value = new Set(changeableItems.value.map((i) => i.currentRelPath));
  }
}

async function handleApply() {
  if (selectedChangeableCount.value === 0 || isApplying.value) return;
  isApplying.value = true;
  try {
    const result = await invoke<RenameApplyResult>('apply_resource_renames', {
      instanceId: props.instanceId,
      restoreOriginal: isRestoreMode.value,
      categories: props.categories ?? null,
      items: Array.from(selectedPaths.value),
    });
    toast.success(
      `已完成 ${result.renamedCount} 项重命名` +
        (result.failed.length > 0 ? `，${result.failed.length} 项失败` : ''),
    );
    emit('applied', result);
    emit('update:open', false);
  } catch (err) {
    toast.error(getErrorMessage(err));
  } finally {
    isApplying.value = false;
  }
}
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-3xl">
    <div class="space-y-4">
      <div>
        <DialogTitle class="flex items-center gap-2 text-lg font-semibold">
          <FolderSync class="w-5 h-5 text-primary" />
          {{ $t('instances.resourceNaming.batchRenameTitle', '批量整理资源文件名') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ $t('instances.resourceNaming.batchRenameDesc', '将当前实例中的 Mod、资源包、光影与数据包按全局命名规则整理，并保留原始文件名以便导出整合包或一键还原。') }}
        </DialogDescription>
      </div>

      <div class="flex flex-wrap items-center justify-between gap-2 border-y border-neutral-200 dark:border-zinc-800 py-2.5">
        <div class="flex items-center gap-2">
          <DButton
            size="sm"
            :variant="!isRestoreMode ? 'primary' : 'outline'"
            @click="isRestoreMode = false"
          >
            {{ $t('instances.resourceNaming.modeApplyRule', '应用命名规则') }}
          </DButton>
          <DButton
            size="sm"
            :variant="isRestoreMode ? 'primary' : 'outline'"
            @click="isRestoreMode = true"
          >
            <RotateCcw class="w-3.5 h-3.5 mr-1.5" />
            {{ $t('instances.resourceNaming.modeRestoreOriginal', '还原原文件名') }}
          </DButton>
        </div>

        <div class="flex items-center gap-3 text-xs">
          <label class="flex items-center gap-1.5 cursor-pointer text-muted-foreground select-none">
            <input type="checkbox" v-model="showUnchanged" class="rounded border-neutral-300 dark:border-zinc-700 accent-emerald-600" />
            {{ $t('instances.resourceNaming.showUnchanged', '显示无需变更项') }}
          </label>
          <DButton
            v-if="changeableItems.length > 0"
            size="sm"
            variant="ghost"
            @click="toggleSelectAll"
          >
            {{
              selectedChangeableCount === changeableItems.length
                ? $t('common.deselectAll', '取消全选')
                : $t('common.selectAll', '全选可变更项')
            }}
          </DButton>
        </div>
      </div>

      <div v-if="isLoadingPreview" class="flex items-center justify-center py-10 text-sm text-muted-foreground gap-2">
        <Loader2 class="w-4 h-4 animate-spin" />
        {{ $t('common.processing', '处理中...') }}
      </div>

      <div v-else-if="visibleItems.length === 0" class="py-10 text-center text-sm text-muted-foreground">
        {{ $t('instances.resourceNaming.noChangesNeeded', '当前实例下的资源文件名均已符合目标格式，无需重命名。') }}
      </div>

      <div v-else class="max-h-80 overflow-y-auto space-y-2 pr-1">
        <div
          v-for="item in visibleItems"
          :key="item.currentRelPath"
          class="flex items-start gap-3 p-2.5 rounded-lg border border-neutral-200 dark:border-zinc-800 bg-neutral-50/70 dark:bg-zinc-800/40 text-xs"
          :class="{ 'opacity-60': !item.willChange }"
        >
          <button
            type="button"
            class="mt-0.5 w-4 h-4 rounded border flex items-center justify-center shrink-0 transition-colors"
            :class="
              selectedPaths.has(item.currentRelPath)
                ? 'bg-emerald-600 border-emerald-600 text-white'
                : 'border-neutral-300 dark:border-zinc-600'
            "
            :disabled="!item.willChange"
            @click="toggleSelection(item.currentRelPath)"
          >
            <Check v-if="selectedPaths.has(item.currentRelPath)" class="w-3 h-3" />
          </button>

          <div class="flex-1 min-w-0 space-y-1">
            <div class="flex items-center gap-2">
              <span class="px-1.5 py-0.5 rounded bg-neutral-200/70 dark:bg-zinc-700 text-[10px] font-medium">
                {{ item.category }}
              </span>
              <span v-if="item.zhName" class="text-emerald-600 dark:text-emerald-400 font-medium truncate">
                {{ item.zhName }}
              </span>
              <span v-if="!item.willChange" class="text-muted-foreground text-[10px]">
                ({{ item.reason === 'missing_zh_fallback' ? '未找到译名，保持原名' : '无需变更' }})
              </span>
            </div>
            <div class="flex items-center gap-1.5 font-mono break-all">
              <span class="text-muted-foreground">{{ item.currentFilename }}</span>
              <ArrowRight v-if="item.willChange" class="w-3.5 h-3.5 text-primary shrink-0" />
              <span v-if="item.willChange" class="text-foreground font-semibold">{{ item.targetFilename }}</span>
            </div>
          </div>
        </div>
      </div>

      <div class="flex items-center justify-between pt-2 border-t border-neutral-200 dark:border-zinc-800">
        <span class="text-xs text-muted-foreground">
          {{ $t('instances.resourceNaming.selectedSummary', { count: selectedChangeableCount, total: changeableItems.length }) }}
        </span>
        <div class="flex items-center gap-2">
          <DButton variant="outline" size="sm" @click="emit('update:open', false)" :disabled="isApplying">
            {{ $t('common.cancel', '取消') }}
          </DButton>
          <DButton
            variant="primary"
            size="sm"
            :disabled="selectedChangeableCount === 0 || isApplying"
            @click="handleApply"
          >
            <Loader2 v-if="isApplying" class="w-3.5 h-3.5 mr-1.5 animate-spin" />
            {{ $t('instances.resourceNaming.applyRename', '立即重命名') }} ({{ selectedChangeableCount }})
          </DButton>
        </div>
      </div>
    </div>
  </DialogContent>
</template>
