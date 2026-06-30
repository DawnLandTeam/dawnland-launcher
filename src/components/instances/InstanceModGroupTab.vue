<template>
  <div class="flex-1 h-full w-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <FolderArchive class="w-5 h-5 text-primary" />
        {{ $t('instances.modGroup', 'Mod Group') }}
      </h3>
    </div>

    <div class="flex-1 overflow-y-auto overflow-x-hidden p-6 flex flex-col gap-4 minimal-scrollbar">
      <div class="flex items-center gap-3">
        <DInput
          v-model="searchQuery"
          :placeholder="$t('instances.searchModGroups', 'Search mod groups...')"
          class="flex-1"
        >
          <template #prefix>
            <Search class="w-4 h-4 text-muted-foreground" />
          </template>
        </DInput>

        <DButton variant="outline" @click="handleOpenFolder">
          <FolderOpen class="w-4 h-4 mr-2" />
          {{ $t('instances.openFolder', 'Open Folder') }}
        </DButton>

        <DButton variant="outline" @click="loadAssets" class="px-3" :disabled="loading">
          <RefreshCw class="w-4 h-4" :class="{ 'animate-spin': loading }" />
        </DButton>
      </div>

      <div v-if="loading" class="flex justify-center items-center py-12">
        <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
      </div>
      
      <div v-else-if="filteredAssets.length === 0" class="flex flex-col items-center justify-center py-16 text-center text-muted-foreground">
        <FolderArchive class="w-16 h-16 mb-4 opacity-20" />
        <p class="text-lg font-medium">{{ $t('instances.noCustomAssets', 'No global custom assets found.') }}</p>
        <p class="text-sm mt-2 max-w-md">{{ $t('instances.customAssetsHint', 'You can place your favorite files in the global_assets folder to quickly apply them to any instance.') }}</p>
      </div>
      
      <div v-else class="grid gap-3">
        <div 
          v-for="asset in filteredAssets" 
          :key="asset.filename"
          class="flex items-center gap-4 p-3 rounded-lg border bg-card text-card-foreground shadow-sm transition-all hover:shadow-md"
        >
          <div class="w-12 h-12 rounded bg-muted flex items-center justify-center overflow-hidden flex-shrink-0 border">
            <FolderArchive v-if="asset.filename.endsWith('.zip')" class="w-6 h-6 text-muted-foreground" />
            <Folder v-else class="w-6 h-6 text-muted-foreground" />
          </div>
          
          <div class="flex-1 min-w-0">
            <div class="font-medium text-sm truncate" :title="asset.filename">{{ asset.filename }}</div>
            <div class="text-xs text-muted-foreground mt-1 flex items-center gap-2">
              <span v-if="asset.isDir" :title="$t('instances.folderSizeSkipped')" class="cursor-help underline decoration-dotted decoration-muted-foreground underline-offset-2">{{ $t('instances.folder') }}</span>
              <span v-else>{{ formatSize(asset.size) }}</span>
            </div>
          </div>
          
          <div class="flex items-center gap-2 flex-shrink-0">
            <DButton 
              variant="primary" 
              size="sm" 
              class="px-3"
              :disabled="applyingAsset === asset.filename"
              @click="applyAsset(asset)"
            >
              <template v-if="applyingAsset === asset.filename">
                <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin mr-1"></div>
                {{ $t('instances.applying', 'Applying...') }}
              </template>
              <template v-else>
                <ArrowRight class="w-4 h-4 mr-1" />
                {{ $t('instances.applyToInstance', 'Apply') }}
              </template>
            </DButton>

            <DButton 
              v-if="asset.filename.endsWith('.json')"
              variant="outline" 
              size="sm" 
              class="px-2"
              :title="$t('instances.editPreset', 'Edit Preset')"
              @click="openEditor(asset)"
            >
              <Edit class="w-4 h-4" />
            </DButton>

            <DButton 
              variant="danger" 
              size="sm" 
              class="px-2"
              @click="confirmDelete(asset)"
            >
              <Trash2 class="w-4 h-4" />
            </DButton>
          </div>
        </div>
      </div>
    </div>

    <!-- Delete Confirmation Alert Dialog -->
    <AlertDialog :open="showDeleteConfirm" @update:open="showDeleteConfirm = $event">
      <div class="fixed inset-0 z-50 bg-black/80 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0" v-if="showDeleteConfirm"></div>
      <div class="fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border bg-background p-6 shadow-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 sm:rounded-lg" v-if="showDeleteConfirm">
        <AlertDialogTitle>{{ $t('instances.deleteConfirm', 'Are you sure you want to delete this?') }}</AlertDialogTitle>
        <AlertDialogDescription>
          {{ $t('instances.deleteGlobalAssetConfirmDesc', 'This action cannot be undone. The global preset file will be permanently deleted.') }}
          <br/><br/>
          <strong class="text-foreground">{{ assetToDelete?.filename }}</strong>
        </AlertDialogDescription>
        <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2 mt-4">
          <DButton variant="outline" @click="showDeleteConfirm = false">{{ $t('instances.cancel', 'Cancel') }}</DButton>
          <DButton variant="danger" @click="executeDelete" :disabled="isDeleting">
             <template v-if="isDeleting">
               <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin mr-2"></div>
             </template>
             {{ $t('instances.delete', 'Delete') }}
          </DButton>
        </div>
      </div>
    </AlertDialog>

    <PresetResolveDialog
      v-if="resolvedData && assetToApply"
      v-model:open="showResolveDialog"
      :instanceId="instanceId"
      :presetName="assetToApply"
      assetType="mod_groups"
      :resolvedData="resolvedData"
      @close="handleResolveDialogClose"
    />

    <PresetEditorDialog
      v-if="assetToEdit"
      v-model:open="showEditorDialog"
      :presetName="assetToEdit"
      :assetType="ASSET_TYPE"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { Search, Trash2, FolderArchive, Folder, ArrowRight, FolderOpen, RefreshCw, Edit } from '@lucide/vue';
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from '../ui/alert-dialog';
import DInput from '../ui/DInput.vue';
import DButton from '../ui/DButton.vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';
import PresetResolveDialog from './PresetResolveDialog.vue';
import PresetEditorDialog from './PresetEditorDialog.vue';

const props = defineProps<{
  instanceId: string;
}>();

const { t } = useI18n();

interface LocalAssetItem {
  filename: string;
  isDir: boolean;
  size: number;
}

const ASSET_TYPE = 'mod_groups';
const assets = ref<LocalAssetItem[]>([]);
const loading = ref(false);
const searchQuery = ref('');

const showDeleteConfirm = ref(false);
const assetToDelete = ref<LocalAssetItem | null>(null);
const isDeleting = ref(false);
const applyingAsset = ref<string | null>(null);

const showResolveDialog = ref(false);
const resolvedData = ref<any>(null);
const assetToApply = ref<string | null>(null);

const showEditorDialog = ref(false);
const assetToEdit = ref<string | null>(null);

const openEditor = (asset: LocalAssetItem) => {
  assetToEdit.value = asset.filename;
  showEditorDialog.value = true;
};

const filteredAssets = computed(() => {
  if (!searchQuery.value) return assets.value;
  const q = searchQuery.value.toLowerCase();
  return assets.value.filter(asset => 
    asset.filename.toLowerCase().includes(q)
  );
});

async function loadAssets() {
  loading.value = true;
  try {
    const result = await invoke<LocalAssetItem[]>('get_custom_assets', { assetType: ASSET_TYPE });
    assets.value = result;
  } catch (error) {
    console.error('Failed to load global assets:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    loading.value = false;
  }
}

watch(() => props.instanceId, () => {
  searchQuery.value = '';
  loadAssets();
}, { immediate: true });

function confirmDelete(asset: LocalAssetItem) {
  assetToDelete.value = asset;
  showDeleteConfirm.value = true;
}

async function executeDelete() {
  if (!assetToDelete.value) return;
  
  isDeleting.value = true;
  try {
    await invoke('delete_custom_asset', {
      assetType: ASSET_TYPE,
      filename: assetToDelete.value.filename,
    });
    
    assets.value = assets.value.filter(a => a.filename !== assetToDelete.value!.filename);
    showDeleteConfirm.value = false;
  } catch (error) {
    console.error('Failed to delete global asset:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    isDeleting.value = false;
  }
}

async function applyAsset(asset: LocalAssetItem) {
  if (!props.instanceId || applyingAsset.value) return;
  
  applyingAsset.value = asset.filename;
  try {
    const result = await invoke('resolve_preset_for_instance', {
      versionId: props.instanceId,
      assetType: ASSET_TYPE,
      presetName: asset.filename
    });
    
    resolvedData.value = result;
    assetToApply.value = asset.filename;
    showResolveDialog.value = true;
  } catch (error) {
    console.error('Failed to resolve preset:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    applyingAsset.value = null;
  }
}

function handleResolveDialogClose() {
  showResolveDialog.value = false;
  resolvedData.value = null;
  assetToApply.value = null;
}

async function handleOpenFolder() {
  try {
    await invoke('open_custom_asset_folder', { assetType: ASSET_TYPE });
  } catch (error) {
    console.error('Failed to open folder:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  }
}

function formatSize(bytes: number) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}
</script>

<style scoped>
.minimal-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.minimal-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(150, 150, 150, 0.3);
  border-radius: 4px;
}
:global(.dark) .minimal-scrollbar::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
}
</style>
