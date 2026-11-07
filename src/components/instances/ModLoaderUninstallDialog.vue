<script setup lang="ts">
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import DButton from '../ui/DButton.vue';
import {
  AlertTriangle,
  Archive,
  FileX,
  FileMinus,
  Check,
  Loader2,
  AlertCircle,
  ShieldAlert,
} from '@lucide/vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';

const props = defineProps<{
  open?: boolean;
  instanceId: string;
  mcVersion: string;
  currentLoaderType?: string;
  currentLoaderVersion?: string;
}>();

const emit = defineEmits<{
  'update:open': [value: boolean];
  'success': [];
}>();

const { t } = useI18n();

type ModHandlingMode = 'disable' | 'backup' | 'keep' | 'delete';

const modHandlingMode = ref<ModHandlingMode>('disable');
const confirmDelete = ref(false);
const submitting = ref(false);
const error = ref<string | null>(null);
const modsCount = ref(0);
const loadingInfo = ref(false);

watch(
  () => props.open,
  async (isOpen) => {
    if (isOpen) {
      error.value = null;
      modHandlingMode.value = 'disable';
      confirmDelete.value = false;
      await fetchLoaderInfo();
    }
  },
  { immediate: true }
);

async function fetchLoaderInfo() {
  if (!props.instanceId) return;
  loadingInfo.value = true;
  try {
    const info = await invoke<{
      mods_count: number;
      has_mods: boolean;
      loader_type: string;
      loader_version?: string;
    }>('get_instance_loader_info', {
      instanceId: props.instanceId,
      mcVersion: props.mcVersion,
    });
    modsCount.value = info.mods_count || 0;
  } catch (e) {
    console.error('Failed to load loader info:', e);
  } finally {
    loadingInfo.value = false;
  }
}

async function handleConfirm() {
  if (submitting.value) return;
  if (modHandlingMode.value === 'delete' && !confirmDelete.value) return;

  submitting.value = true;
  error.value = null;

  try {
    await invoke('uninstall_instance_loader', {
      instanceId: props.instanceId,
      modHandlingMode: modHandlingMode.value,
      mcVersion: props.mcVersion,
    });
    toast.success(
      t('instances.loaderManagement.uninstallSuccess', 'Mod loader uninstallation task submitted')
    );
    emit('success');
    emit('update:open', false);
  } catch (e) {
    console.error('Failed to uninstall loader:', e);
    error.value = getErrorMessage(e);
    toast.error(t('common.operationFailed', 'Operation failed'), getErrorMessage(e));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-lg">
    <div class="space-y-4">
      <div>
        <DialogTitle class="flex items-center gap-2 text-lg font-bold text-rose-600 dark:text-rose-400">
          <ShieldAlert class="h-5 w-5 shrink-0" />
          {{ t('instances.loaderManagement.uninstallDialogTitle', 'Uninstall Mod Loader') }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ instanceId }} · Minecraft {{ mcVersion }} · {{ currentLoaderType || 'Loader' }} {{ currentLoaderVersion ? `(${currentLoaderVersion})` : '' }}
        </DialogDescription>
      </div>

      <!-- Prominent Warning Card -->
      <div class="p-3.5 rounded-lg border border-amber-500/30 bg-amber-500/10 text-xs space-y-1.5">
        <div class="flex items-center gap-2 font-semibold text-amber-700 dark:text-amber-400">
          <AlertTriangle class="h-4 w-4 shrink-0" />
          <span>{{ t('instances.loaderManagement.uninstallWarningTitle', 'Warning: Reverting to Pure Vanilla') }}</span>
        </div>
        <p class="text-amber-800/90 dark:text-amber-200/90 leading-relaxed">
          {{
            t(
              'instances.loaderManagement.uninstallWarningDesc',
              'After uninstalling the mod loader, this instance will revert to pure vanilla Minecraft. Opening modded worlds in pure vanilla may cause custom mod items, blocks, and entities to be permanently lost or corrupted!'
            )
          }}
        </p>
      </div>

      <!-- Mod Handling Options -->
      <div class="space-y-2">
        <label class="text-xs font-semibold text-neutral-800 dark:text-neutral-200 flex items-center justify-between">
          <span>{{ t('instances.loaderManagement.modHandlingTitle', 'How to handle existing mod files?') }}</span>
          <span v-if="modsCount > 0" class="text-[11px] font-normal text-muted-foreground">
            {{ modsCount }} mods detected
          </span>
        </label>

        <div class="space-y-2">
          <!-- Option: Disable -->
          <label
            class="flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-all text-xs"
            :class="[
              modHandlingMode === 'disable'
                ? 'border-primary bg-primary/5 ring-1 ring-primary'
                : 'border-neutral-200 dark:border-zinc-800 hover:bg-neutral-50 dark:hover:bg-zinc-800/40',
            ]"
          >
            <input
              type="radio"
              name="mod_handling"
              value="disable"
              v-model="modHandlingMode"
              class="mt-0.5 text-primary focus:ring-primary h-3.5 w-3.5"
            />
            <div class="flex-1 space-y-0.5">
              <div class="font-medium text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                <FileMinus class="h-3.5 w-3.5 text-primary" />
                <span>{{ t('instances.loaderManagement.modHandlingDisable', 'Disable all mods (Recommended)') }}</span>
              </div>
              <p class="text-muted-foreground text-[11px] leading-normal">
                {{
                  t(
                    'instances.loaderManagement.modHandlingDisableDesc',
                    'Rename .jar mod files to .disable. They remain safely in the mods directory and can be restored if a loader is reinstalled.'
                  )
                }}
              </p>
            </div>
          </label>

          <!-- Option: Backup -->
          <label
            class="flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-all text-xs"
            :class="[
              modHandlingMode === 'backup'
                ? 'border-primary bg-primary/5 ring-1 ring-primary'
                : 'border-neutral-200 dark:border-zinc-800 hover:bg-neutral-50 dark:hover:bg-zinc-800/40',
            ]"
          >
            <input
              type="radio"
              name="mod_handling"
              value="backup"
              v-model="modHandlingMode"
              class="mt-0.5 text-primary focus:ring-primary h-3.5 w-3.5"
            />
            <div class="flex-1 space-y-0.5">
              <div class="font-medium text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
                <Archive class="h-3.5 w-3.5 text-blue-500" />
                <span>{{ t('instances.loaderManagement.modHandlingBackup', 'Backup and move mods') }}</span>
              </div>
              <p class="text-muted-foreground text-[11px] leading-normal">
                {{
                  t(
                    'instances.loaderManagement.modHandlingBackupDesc',
                    'Rename the mods folder to mods_backup_<timestamp>.'
                  )
                }}
              </p>
            </div>
          </label>

          <!-- Option: Keep -->
          <label
            class="flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-all text-xs"
            :class="[
              modHandlingMode === 'keep'
                ? 'border-primary bg-primary/5 ring-1 ring-primary'
                : 'border-neutral-200 dark:border-zinc-800 hover:bg-neutral-50 dark:hover:bg-zinc-800/40',
            ]"
          >
            <input
              type="radio"
              name="mod_handling"
              value="keep"
              v-model="modHandlingMode"
              class="mt-0.5 text-primary focus:ring-primary h-3.5 w-3.5"
            />
            <div class="flex-1 space-y-0.5">
              <div class="font-medium text-neutral-900 dark:text-neutral-100">
                {{ t('instances.loaderManagement.modHandlingKeep', 'Keep mod files') }}
              </div>
              <p class="text-muted-foreground text-[11px] leading-normal">
                {{
                  t(
                    'instances.loaderManagement.modHandlingKeepDesc',
                    'Keep the mods folder as is. Pure vanilla Minecraft will simply ignore the folder.'
                  )
                }}
              </p>
            </div>
          </label>

          <!-- Option: Delete -->
          <label
            class="flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-all text-xs"
            :class="[
              modHandlingMode === 'delete'
                ? 'border-rose-500 bg-rose-500/5 ring-1 ring-rose-500'
                : 'border-neutral-200 dark:border-zinc-800 hover:bg-neutral-50 dark:hover:bg-zinc-800/40',
            ]"
          >
            <input
              type="radio"
              name="mod_handling"
              value="delete"
              v-model="modHandlingMode"
              class="mt-0.5 text-rose-500 focus:ring-rose-500 h-3.5 w-3.5"
            />
            <div class="flex-1 space-y-0.5">
              <div class="font-medium text-rose-600 dark:text-rose-400 flex items-center gap-1.5">
                <FileX class="h-3.5 w-3.5 text-rose-500" />
                <span>{{ t('instances.loaderManagement.modHandlingDelete', 'Permanently delete mod files') }}</span>
              </div>
              <p class="text-muted-foreground text-[11px] leading-normal">
                {{
                  t(
                    'instances.loaderManagement.modHandlingDeleteDesc',
                    'Permanently delete the mods folder and all files inside. Cannot be undone!'
                  )
                }}
              </p>
            </div>
          </label>
        </div>
      </div>

      <!-- Extra confirmation checkbox when Delete is chosen -->
      <div
        v-if="modHandlingMode === 'delete'"
        class="p-2.5 rounded-lg border border-rose-300 dark:border-rose-900 bg-rose-50/50 dark:bg-rose-950/20 text-xs flex items-start gap-2.5"
      >
        <input
          type="checkbox"
          id="confirmDeleteCheck"
          v-model="confirmDelete"
          class="mt-0.5 rounded border-rose-300 text-rose-600 focus:ring-rose-500 h-3.5 w-3.5"
        />
        <label for="confirmDeleteCheck" class="text-rose-700 dark:text-rose-300 text-[11px] cursor-pointer leading-tight select-none">
          {{
            t(
              'instances.loaderManagement.modHandlingDeleteConfirm',
              'I understand that deleting mod files is irreversible, and worlds relying on them may be broken.'
            )
          }}
        </label>
      </div>

      <!-- Error Message -->
      <div
        v-if="error"
        class="p-2.5 text-xs text-rose-600 bg-rose-50 dark:bg-rose-950/30 rounded border border-rose-200 dark:border-rose-900 flex items-start gap-2"
      >
        <AlertCircle class="h-4 w-4 shrink-0 mt-0.5" />
        <span class="break-all">{{ error }}</span>
      </div>

      <!-- Footer Buttons -->
      <div class="flex items-center justify-end gap-2 pt-2 border-t border-neutral-100 dark:border-zinc-800">
        <DButton
          variant="outline"
          size="sm"
          @click="emit('update:open', false)"
          :disabled="submitting"
        >
          {{ t('common.cancel', 'Cancel') }}
        </DButton>
        <DButton
          variant="primary"
          size="sm"
          @click="handleConfirm"
          :disabled="submitting || (modHandlingMode === 'delete' && !confirmDelete)"
          class="min-w-24 !bg-rose-600 hover:!bg-rose-700 text-white"
        >
          <template v-if="submitting">
            <Loader2 class="h-4 w-4 animate-spin mr-1.5" />
            {{ t('common.processing', 'Processing...') }}
          </template>
          <template v-else>
            <Check class="h-4 w-4 mr-1.5" />
            {{ t('instances.loaderManagement.uninstallConfirmBtn', 'Confirm Uninstall') }}
          </template>
        </DButton>
      </div>
    </div>
  </DialogContent>
</template>
