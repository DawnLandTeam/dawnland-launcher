<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useI18n } from 'vue-i18n';
import {
  DialogContent,
  DialogTitle,
  DialogDescription,
} from '../ui/dialog';
import DButton from '../ui/DButton.vue';
import DSelect from '../ui/DSelect.vue';
import { Download, ArrowUpDown, Loader2, Check, AlertCircle } from '@lucide/vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';

const props = defineProps<{
  open?: boolean;
  instanceId: string;
  mcVersion: string;
  currentLoaderType?: string;
  currentLoaderVersion?: string;
  mode: 'install' | 'switch';
}>();

const emit = defineEmits<{
  'update:open': [value: boolean];
  'success': [];
}>();

const { t } = useI18n();

interface LoaderVersionItem {
  version: string;
  isStable?: boolean;
}

const selectedLoader = ref<'fabric' | 'forge' | 'neoforge'>('fabric');
const selectedVersion = ref('');
const loadingLoaders = ref(false);
const submitting = ref(false);
const error = ref<string | null>(null);

const availableVersions = ref<LoaderVersionItem[]>([]);

const loaderTypes = [
  { id: 'fabric', name: 'Fabric', icon: '/fabric.png' },
  { id: 'forge', name: 'Forge', icon: '/forge.png' },
  { id: 'neoforge', name: 'NeoForge', icon: '/neoforge.png' },
] as const;

let loadRequestId = 0;

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      error.value = null;
      if (props.mode === 'switch' && props.currentLoaderType) {
        const lower = props.currentLoaderType.toLowerCase();
        if (lower === 'forge') selectedLoader.value = 'forge';
        else if (lower === 'neoforge') selectedLoader.value = 'neoforge';
        else selectedLoader.value = 'fabric';
      } else {
        selectedLoader.value = 'fabric';
      }
      loadLoaders();
    }
  },
  { immediate: true }
);

function selectLoader(loaderId: 'fabric' | 'forge' | 'neoforge') {
  if (selectedLoader.value === loaderId) return;
  selectedLoader.value = loaderId;
  if (props.open) {
    loadLoaders();
  }
}

function normalizeVersionForMatch(version: string): string {
  const trimmed = version.trim();
  const mcPrefix = props.mcVersion ? `${props.mcVersion.trim()}-` : '';
  if (mcPrefix && trimmed.startsWith(mcPrefix)) {
    return trimmed.slice(mcPrefix.length);
  }
  return trimmed;
}

function isVersionMatch(candidate: string, current?: string): boolean {
  if (!current) return false;
  const trimmedCandidate = candidate.trim();
  const trimmedCurrent = current.trim();
  if (trimmedCandidate === trimmedCurrent) return true;
  return normalizeVersionForMatch(trimmedCandidate) === normalizeVersionForMatch(trimmedCurrent);
}

async function loadLoaders() {
  if (!props.mcVersion) return;
  const currentReqId = ++loadRequestId;
  const targetLoader = selectedLoader.value;
  loadingLoaders.value = true;
  error.value = null;
  availableVersions.value = [];
  selectedVersion.value = '';

  try {
    let list: LoaderVersionItem[] = [];
    if (targetLoader === 'fabric') {
      const res = await invoke<{ stable: string[]; unstable: string[] }>('get_fabric_loaders', {
        mcVersion: props.mcVersion,
      });
      list = [
        ...res.stable.map((v) => ({ version: v, isStable: true })),
        ...res.unstable.map((v) => ({ version: v, isStable: false })),
      ];
    } else if (targetLoader === 'forge') {
      const res = await invoke<{ versions: Array<{ version: string }> }>('get_forge_loaders', {
        mcVersion: props.mcVersion,
      });
      list = res.versions.map((v) => ({ version: v.version, isStable: true }));
    } else if (targetLoader === 'neoforge') {
      const res = await invoke<{ versions: Array<{ version: string }> }>('get_neoforge_loaders', {
        mcVersion: props.mcVersion,
      });
      list = res.versions.map((v) => ({ version: v.version, isStable: true }));
    }

    if (currentReqId !== loadRequestId || selectedLoader.value !== targetLoader) {
      return;
    }

    availableVersions.value = list;

    if (availableVersions.value.length > 0) {
      // Default to first version, or current if found
      if (
        props.mode === 'switch' &&
        props.currentLoaderVersion &&
        props.currentLoaderType?.toLowerCase() === targetLoader
      ) {
        const matched = availableVersions.value.find((v) =>
          isVersionMatch(v.version, props.currentLoaderVersion)
        );
        selectedVersion.value = matched ? matched.version : availableVersions.value[0].version;
      } else {
        selectedVersion.value = availableVersions.value[0].version;
      }
    }
  } catch (e) {
    if (currentReqId !== loadRequestId || selectedLoader.value !== targetLoader) {
      return;
    }
    console.error('Failed to load loader versions:', e);
    error.value = getErrorMessage(e);
  } finally {
    if (currentReqId === loadRequestId) {
      loadingLoaders.value = false;
    }
  }
}

function compareVersions(target: string, current?: string): 'same' | 'upgrade' | 'downgrade' {
  if (!current || isVersionMatch(target, current)) return 'same';
  // Check index in availableVersions list (index 0 is newest)
  const targetIdx = availableVersions.value.findIndex((v) => v.version === target);
  const currentIdx = availableVersions.value.findIndex((v) => isVersionMatch(v.version, current));
  if (targetIdx !== -1 && currentIdx !== -1) {
    return targetIdx < currentIdx ? 'upgrade' : 'downgrade';
  }
  return 'same';
}

const versionChangeType = computed(() => {
  if (props.mode !== 'switch' || !props.currentLoaderVersion) return null;
  return compareVersions(selectedVersion.value, props.currentLoaderVersion);
});

const selectOptions = computed(() => {
  return availableVersions.value.map((item, idx) => {
    let suffix = '';
    if (idx === 0) {
      suffix += ` [${t('instances.loaderManagement.latestStableBadge', 'Latest')}]`;
    }
    if (
      props.mode === 'switch' &&
      props.currentLoaderVersion &&
      isVersionMatch(item.version, props.currentLoaderVersion)
    ) {
      suffix += ` (${t('instances.loaderManagement.currentVersionBadge', 'Current')})`;
    }
    return {
      label: `${item.version}${suffix}`,
      value: item.version,
    };
  });
});

async function handleConfirm() {
  if (!selectedVersion.value || submitting.value) return;
  submitting.value = true;
  error.value = null;

  try {
    if (props.mode === 'install') {
      await invoke('install_instance_loader', {
        instanceId: props.instanceId,
        loaderType: selectedLoader.value,
        loaderVersion: selectedVersion.value,
        mcVersion: props.mcVersion,
      });
      toast.success(
        t('instances.loaderManagement.operationSuccess', 'Task submitted and running in background')
      );
    } else {
      await invoke('switch_instance_loader_version', {
        instanceId: props.instanceId,
        loaderType: selectedLoader.value,
        loaderVersion: selectedVersion.value,
        mcVersion: props.mcVersion,
      });
      toast.success(
        t('instances.loaderManagement.operationSuccess', 'Task submitted and running in background')
      );
    }
    emit('success');
    emit('update:open', false);
  } catch (e) {
    console.error('Failed to install or switch loader:', e);
    error.value = getErrorMessage(e);
    toast.error(t('common.operationFailed', 'Operation failed'), getErrorMessage(e));
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <DialogContent :open="open" @update:open="emit('update:open', $event)" class="max-w-md">
    <div class="space-y-4">
      <div>
        <DialogTitle class="flex items-center gap-2 text-lg font-bold">
          <Download v-if="mode === 'install'" class="h-5 w-5 text-primary" />
          <ArrowUpDown v-else class="h-5 w-5 text-primary" />
          {{
            mode === 'install'
              ? t('instances.loaderManagement.installDialogTitle', 'Install Mod Loader')
              : t('instances.loaderManagement.switchDialogTitle', 'Switch Mod Loader Version')
          }}
        </DialogTitle>
        <DialogDescription class="text-xs text-muted-foreground mt-1">
          {{ instanceId }} · Minecraft {{ mcVersion }}
        </DialogDescription>
      </div>

      <!-- Select Loader Type (Only visible in 'install' mode) -->
      <div v-if="mode === 'install'" class="space-y-2">
        <label class="text-xs font-semibold text-neutral-700 dark:text-neutral-300">
          {{ t('instances.loaderManagement.selectLoader', 'Select Mod Loader') }}
        </label>
        <div class="grid grid-cols-3 gap-2">
          <button
            v-for="loader in loaderTypes"
            :key="loader.id"
            type="button"
            @click="selectLoader(loader.id)"
            class="flex flex-col items-center gap-1.5 p-3 rounded-lg border text-xs font-medium transition-all"
            :class="[
              selectedLoader === loader.id
                ? 'border-primary bg-primary/10 text-primary ring-1 ring-primary'
                : 'border-neutral-200 dark:border-zinc-800 hover:bg-neutral-50 dark:hover:bg-zinc-800/50 text-neutral-700 dark:text-neutral-300',
            ]"
          >
            <img :src="loader.icon" class="w-6 h-6 object-contain" />
            <span>{{ loader.name }}</span>
          </button>
        </div>
      </div>

      <!-- Current Version Display in 'switch' mode -->
      <div
        v-else
        class="flex items-center justify-between p-3 rounded-lg bg-neutral-50 dark:bg-zinc-800/50 border border-neutral-200 dark:border-zinc-800 text-xs"
      >
        <span class="text-muted-foreground">{{ t('instances.loaderManagement.currentLoader', 'Current Loader') }}:</span>
        <span class="font-semibold text-neutral-900 dark:text-neutral-100">
          {{ selectedLoader.toUpperCase() }} {{ currentLoaderVersion || 'Unknown' }}
        </span>
      </div>

      <!-- Select Loader Version -->
      <div class="space-y-2">
        <div class="flex items-center justify-between">
          <label class="text-xs font-semibold text-neutral-700 dark:text-neutral-300">
            {{ t('instances.loaderManagement.selectVersion', 'Select Version') }}
          </label>
          <span
            v-if="versionChangeType && versionChangeType !== 'same'"
            class="text-[10px] px-2 py-0.5 rounded-full font-bold uppercase"
            :class="[
              versionChangeType === 'upgrade'
                ? 'bg-emerald-100 dark:bg-emerald-950/50 text-emerald-600 dark:text-emerald-400 border border-emerald-300 dark:border-emerald-800'
                : 'bg-amber-100 dark:bg-amber-950/50 text-amber-600 dark:text-amber-400 border border-amber-300 dark:border-amber-800',
            ]"
          >
            {{ versionChangeType === 'upgrade' ? t('instances.loaderManagement.upgrade', 'Upgrade') : t('instances.loaderManagement.downgrade', 'Downgrade') }}
          </span>
        </div>

        <div v-if="loadingLoaders" class="flex items-center justify-center py-6 text-muted-foreground text-xs gap-2">
          <Loader2 class="h-4 w-4 animate-spin text-primary" />
          {{ t('instances.loaderManagement.loadingVersions', 'Loading available versions...') }}
        </div>

        <div v-else-if="availableVersions.length === 0" class="p-3 text-xs text-amber-600 bg-amber-50 dark:bg-amber-950/30 rounded border border-amber-200 dark:border-amber-900 flex items-center gap-2">
          <AlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ t('instances.noVersionsFound', 'No compatible versions found for this Minecraft version.') }}</span>
        </div>

        <DSelect
          v-else
          v-model="selectedVersion"
          :options="selectOptions"
          searchable
          :search-placeholder="t('instances.loaderManagement.searchVersions', 'Search versions...')"
          :empty-text="t('instances.loaderManagement.noMatchingVersions', 'No matching versions')"
          class="w-full"
        />
      </div>

      <!-- Error Message -->
      <div v-if="error" class="p-2.5 text-xs text-rose-600 bg-rose-50 dark:bg-rose-950/30 rounded border border-rose-200 dark:border-rose-900 flex items-start gap-2">
        <AlertCircle class="h-4 w-4 shrink-0 mt-0.5" />
        <span class="break-all">{{ error }}</span>
      </div>

      <!-- Dialog Footer -->
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
          :disabled="!selectedVersion || loadingLoaders || submitting"
          class="min-w-20"
        >
          <template v-if="submitting">
            <Loader2 class="h-4 w-4 animate-spin mr-1.5" />
            {{ t('common.submitting', 'Submitting...') }}
          </template>
          <template v-else>
            <Check class="h-4 w-4 mr-1.5" />
            {{ t('common.confirm', 'Confirm') }}
          </template>
        </DButton>
      </div>
    </div>
  </DialogContent>
</template>
