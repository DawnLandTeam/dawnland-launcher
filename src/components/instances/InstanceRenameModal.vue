<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import { Pencil, Loader2 } from '@lucide/vue';

import {
  AlertDialog,
  AlertDialogDescription,
  AlertDialogTitle,
} from '../ui/alert-dialog';
import DInput from '../ui/DInput.vue';
import DButton from '../ui/DButton.vue';

import { useInstances } from '../../composables/useInstances';
import { useStatistics } from '../../composables/useStatistics';
import { renameCachedInstanceIcon } from '../../composables/useInstanceIconUpdates';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';
import { isSafeInstanceName } from '../../utils/instanceName';

const props = defineProps<{
  open: boolean;
  instanceId: string;
  instanceName?: string;
}>();

const emit = defineEmits<{
  'update:open': [value: boolean];
  renamed: [payload: { oldId: string; newId: string }];
}>();

const { t } = useI18n();
const { instances, fetchInstances } = useInstances();
const { fetchAllStats } = useStatistics();

const newName = ref('');
const errorMsg = ref<string | null>(null);
const isRenaming = ref(false);
const formRef = ref<HTMLFormElement | null>(null);

watch(
  () => props.open,
  async (isOpen) => {
    if (isOpen) {
      newName.value = props.instanceName || props.instanceId;
      errorMsg.value = null;
      await nextTick();
      const input = formRef.value?.querySelector('input');
      if (input) {
        input.focus();
        input.select();
      }
    }
  },
  { immediate: true },
);

function handleOpenChange(val: boolean) {
  if (!val && isRenaming.value) return;
  emit('update:open', val);
}

function handleNameInput(val: string) {
  newName.value = val;
  if (errorMsg.value) {
    errorMsg.value = null;
  }
}

async function handleSubmit() {
  if (isRenaming.value || !props.instanceId) return;

  const trimmed = newName.value.trim();
  if (!trimmed) {
    errorMsg.value = t('instances.renameDialog.emptyName', 'Please enter an instance name');
    return;
  }

  if (trimmed === props.instanceId) {
    emit('update:open', false);
    return;
  }

  if (!isSafeInstanceName(trimmed)) {
    errorMsg.value = t(
      'instances.renameDialog.invalidName',
      'Instance name contains invalid characters or reserved system names',
    );
    return;
  }

  const hasConflict = instances.value.some(
    (inst) => inst.id !== props.instanceId && inst.id.toLowerCase() === trimmed.toLowerCase(),
  );
  if (hasConflict) {
    errorMsg.value = t(
      'instances.renameDialog.alreadyExists',
      'An instance with this name already exists',
    );
    return;
  }

  isRenaming.value = true;
  errorMsg.value = null;

  try {
    const oldId = props.instanceId;
    await invoke('rename_instance', {
      oldId,
      newId: trimmed,
    });

    if (typeof window !== 'undefined') {
      if (localStorage.getItem('selectedInstanceId') === oldId) {
        localStorage.setItem('selectedInstanceId', trimmed);
      }
      window.dispatchEvent(
        new CustomEvent('instance-renamed', {
          detail: { oldId, newId: trimmed },
        }),
      );
    }

    renameCachedInstanceIcon(oldId, trimmed);
    await Promise.all([fetchInstances(true), fetchAllStats()]);

    toast.success(t('instances.renameDialog.success', { name: trimmed }));
    emit('renamed', { oldId, newId: trimmed });
    emit('update:open', false);
  } catch (e) {
    console.error('Failed to rename instance:', e);
    errorMsg.value = getErrorMessage(e);
    toast.error(
      t('instances.renameDialog.failed', 'Failed to rename instance'),
      errorMsg.value,
    );
  } finally {
    isRenaming.value = false;
  }
}
</script>

<template>
  <AlertDialog :open="open" @update:open="handleOpenChange">
    <AlertDialogTitle class="flex items-center gap-2 text-lg font-semibold text-neutral-900 dark:text-white pr-6">
      <Pencil class="h-4 w-4 text-primary shrink-0" />
      <span>{{ t('instances.renameDialog.title', 'Rename Instance') }}</span>
    </AlertDialogTitle>
    <AlertDialogDescription class="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
      {{ t('instances.renameDialog.desc', 'Enter a new name for this instance. Its folder and internal configuration files will be updated.') }}
    </AlertDialogDescription>

    <form ref="formRef" @submit.prevent="handleSubmit" class="mt-4 space-y-4">
      <div class="space-y-2">
        <label class="text-xs font-medium text-neutral-700 dark:text-neutral-300">
          {{ t('instances.renameDialog.label', 'Instance Name') }}
        </label>
        <DInput
          :model-value="newName"
          @update:model-value="handleNameInput"
          :placeholder="t('instances.renameDialog.placeholder', 'Enter new instance name')"
          :disabled="isRenaming"
        />
        <p v-if="errorMsg" class="text-xs text-red-600 dark:text-red-400">
          {{ errorMsg }}
        </p>
      </div>

      <div class="flex justify-end gap-2 pt-2">
        <DButton
          type="button"
          variant="outline"
          size="sm"
          :disabled="isRenaming"
          @click="handleOpenChange(false)"
        >
          {{ t('common.cancel', 'Cancel') }}
        </DButton>
        <DButton
          type="submit"
          variant="primary"
          size="sm"
          :disabled="isRenaming || !newName.trim()"
        >
          <Loader2 v-if="isRenaming" class="h-3.5 w-3.5 mr-1.5 animate-spin" />
          {{ t('common.confirm', 'Confirm') }}
        </DButton>
      </div>
    </form>
  </AlertDialog>
</template>
