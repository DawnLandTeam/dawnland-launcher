<template>
  <div class="flex-1 h-full w-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <Globe class="w-5 h-5 text-primary" />
        {{ $t('downloadsCenter.tabs.world', 'Worlds') }}
      </h3>
    </div>

    <div class="flex-1 overflow-y-auto overflow-x-hidden p-6 flex flex-col gap-4 minimal-scrollbar">
      <div class="flex items-center gap-3">
        <DInput
          v-model="searchQuery"
          :placeholder="$t('instances.searchWorlds', 'Search installed worlds...')"
          class="flex-1"
        >
          <template #prefix>
            <Search class="w-4 h-4 text-muted-foreground" />
          </template>
        </DInput>
        
        <!-- We don't have download more worlds yet, but let's keep the button disabled or redirect to an empty page if needed. Let's redirect to downloads? The downloads center doesn't have a world tab yet. So let's hide it for now -->
      </div>

      <div v-if="loading" class="flex justify-center items-center py-12">
        <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
      </div>
      
      <div v-else-if="filteredAssets.length === 0" class="flex flex-col items-center justify-center py-16 text-center text-muted-foreground">
        <Globe class="w-16 h-16 mb-4 opacity-20" />
        <p class="text-lg font-medium">{{ $t('instances.noWorlds', 'No worlds installed for this instance') }}</p>
      </div>
      
      <div v-else class="grid gap-3">
        <div 
          v-for="asset in filteredAssets" 
          :key="asset.filename"
          class="flex items-center gap-4 p-3 rounded-lg border bg-card text-card-foreground shadow-sm transition-all hover:shadow-md"
        >
          <div class="w-12 h-12 rounded bg-muted flex items-center justify-center overflow-hidden flex-shrink-0 border">
            <Globe class="w-6 h-6 text-muted-foreground" />
          </div>
          
          <div class="flex-1 min-w-0">
            <div class="font-medium text-sm truncate" :title="asset.filename">{{ asset.filename }}</div>
            <div class="text-xs text-muted-foreground mt-1 flex items-center gap-2">
              <span>{{ asset.isDir ? 'Folder' : formatSize(asset.size) }}</span>
            </div>
          </div>
          
          <div class="flex items-center gap-2 flex-shrink-0">
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
          {{ $t('instances.deleteConfirmDesc', 'This action cannot be undone. The file will be permanently deleted.') }}
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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { invoke } from '@tauri-apps/api/core';
import { Globe, Search, Trash2 } from '@lucide/vue';
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from '../ui/alert-dialog';
import DInput from '../ui/DInput.vue';
import DButton from '../ui/DButton.vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';

const props = defineProps<{
  instanceId: string;
}>();

const { t } = useI18n();

interface LocalAssetItem {
  filename: string;
  isDir: boolean;
  size: number;
}

const assets = ref<LocalAssetItem[]>([]);
const loading = ref(false);
const searchQuery = ref('');

const showDeleteConfirm = ref(false);
const assetToDelete = ref<LocalAssetItem | null>(null);
const isDeleting = ref(false);

const filteredAssets = computed(() => {
  if (!searchQuery.value) return assets.value;
  const q = searchQuery.value.toLowerCase();
  return assets.value.filter(asset => 
    asset.filename.toLowerCase().includes(q)
  );
});

async function loadAssets() {
  if (!props.instanceId) return;
  
  loading.value = true;
  try {
    const result = await invoke<LocalAssetItem[]>('get_installed_worlds', { versionId: props.instanceId });
    assets.value = result;
  } catch (error) {
    console.error('Failed to load worlds:', error);
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
    await invoke('delete_local_world', {
      versionId: props.instanceId,
      worldName: assetToDelete.value.filename,
    });
    
    assets.value = assets.value.filter(a => a.filename !== assetToDelete.value!.filename);
    showDeleteConfirm.value = false;
  } catch (error) {
    console.error('Failed to delete world:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    isDeleting.value = false;
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
