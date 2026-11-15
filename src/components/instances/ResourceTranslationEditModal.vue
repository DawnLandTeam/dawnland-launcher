<script setup lang="ts">
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Languages, Loader2 } from '@lucide/vue';
import { DialogContent, DialogDescription, DialogTitle } from '../ui/dialog';
import DButton from '../ui/DButton.vue';
import DInput from '../ui/DInput.vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';
import type { AssetCategory, TranslationLookupResult } from '../../types/resourceNaming';

export interface TranslationEditTarget {
  category: AssetCategory;
  instanceId: string;
  relPath: string;
  filename: string;
  source?: string | null;
  projectId?: string | null;
  slug?: string | null;
  rawName?: string | null;
  currentZhName?: string | null;
}

const props = defineProps<{
  open: boolean;
  target: TranslationEditTarget | null;
}>();

const emit = defineEmits<{
  (e: 'update:open', value: boolean): void;
  (e: 'saved', result: TranslationLookupResult): void;
}>();

const zhInput = ref('');
const submitFeedback = ref(true);
const isSaving = ref(false);

watch(
  () => [props.open, props.target] as const,
  ([isOpen, targetVal]) => {
    if (isOpen && targetVal) {
      zhInput.value = targetVal.currentZhName || '';
      submitFeedback.value = true;
    }
  },
);

async function handleSave() {
  if (!props.target || isSaving.value) return;
  const trimmed = zhInput.value.trim();
  if (!trimmed) {
    toast.error(getErrorMessage('请输入中文名称'));
    return;
  }

  isSaving.value = true;
  try {
    const result = await invoke<TranslationLookupResult>('set_custom_resource_translation', {
      category: props.target.category,
      source: props.target.source ?? null,
      projectId: props.target.projectId ?? null,
      slug: props.target.slug ?? null,
      rawName: props.target.rawName ?? props.target.filename,
      zhName: trimmed,
      instanceId: props.target.instanceId,
      relPath: props.target.relPath,
      submitFeedback: submitFeedback.value,
    });
    toast.success('已保存自定义译名');
    emit('saved', result);
    emit('update:open', false);
  } catch (err) {
    toast.error(getErrorMessage(err));
  } finally {
    isSaving.value = false;
  }
}
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-md">
    <div class="space-y-4">
      <div>
        <DialogTitle class="flex items-center gap-2 text-base font-semibold">
          <Languages class="w-4 h-4 text-primary" />
          {{ $t('instances.resourceNaming.editTranslationTitle', '自定义中文译名') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1 break-all">
          {{ target?.rawName || target?.filename }}
        </DialogDescription>
      </div>

      <div class="space-y-2">
        <label class="text-xs font-medium text-foreground block">
          {{ $t('instances.resourceNaming.customZhLabel', '中文译名 / 别名') }}
        </label>
        <DInput
          v-model="zhInput"
          :placeholder="$t('instances.resourceNaming.customZhPlaceholder', '例如：机械动力 (Create)')"
          @keydown.enter="handleSave"
        />
        <p class="text-[11px] text-muted-foreground">
          {{ $t('instances.resourceNaming.customZhHint', '用户自定义译名拥有最高优先级，不会被后续自动同步覆盖；重命名文件名时会自动截取括号前短名。') }}
        </p>
      </div>

      <label class="flex items-center gap-2 text-xs text-muted-foreground cursor-pointer select-none">
        <input
          type="checkbox"
          v-model="submitFeedback"
          class="rounded border-neutral-300 dark:border-zinc-700 accent-emerald-600"
        />
        {{ $t('instances.resourceNaming.submitFeedbackOption', '同时提交至社区词库审核队列，帮助改进大众统一译名') }}
      </label>

      <div class="flex justify-end gap-2 pt-2">
        <DButton variant="outline" size="sm" @click="emit('update:open', false)" :disabled="isSaving">
          {{ $t('common.cancel', '取消') }}
        </DButton>
        <DButton variant="primary" size="sm" :disabled="!zhInput.trim() || isSaving" @click="handleSave">
          <Loader2 v-if="isSaving" class="w-3.5 h-3.5 mr-1.5 animate-spin" />
          {{ $t('common.save', '保存') }}
        </DButton>
      </div>
    </div>
  </DialogContent>
</template>
