<template>
  <div class="flex-1 h-full w-full flex flex-col min-h-0 bg-white/40 dark:bg-zinc-900/40">
    <div class="px-6 py-4 border-b border-neutral-200/50 dark:border-zinc-800/50 flex-shrink-0 flex items-center justify-between">
      <h3 class="text-lg font-semibold flex items-center gap-2">
        <Package class="w-5 h-5 text-primary" />
        {{ $t('instances.localModsTitle', 'Local Mods Management') }}
      </h3>
    </div>

    <div class="flex-1 overflow-y-auto overflow-x-hidden p-6 flex flex-col gap-4 minimal-scrollbar">
      <div class="flex items-center gap-3">
        <DInput
          v-model="searchQuery"
          :placeholder="$t('instances.searchLocalMods', 'Search installed mods...')"
          class="flex-1"
        >
          <template #prefix>
            <Search class="w-4 h-4 text-muted-foreground" />
          </template>
        </DInput>
        
        <DButton variant="primary" @click="handleDownloadMore">
          <Download class="w-4 h-4 mr-2" />
          {{ $t('instances.downloadMoreMods', 'Download More Mods') }}
        </DButton>
      </div>

      <div v-if="loading" class="flex justify-center items-center py-12">
        <div class="w-8 h-8 border-4 border-primary border-t-transparent rounded-full animate-spin"></div>
      </div>
      
      <div v-else-if="filteredMods.length === 0" class="flex flex-col items-center justify-center py-16 text-center text-muted-foreground">
        <Package class="w-16 h-16 mb-4 opacity-20" />
        <p class="text-lg font-medium">{{ $t('instances.noLocalMods', 'No mods installed for this instance') }}</p>
      </div>
      
      <div v-else class="grid gap-3">
        <div 
          v-for="mod in filteredMods" 
          :key="mod.filename"
          class="flex items-center gap-4 p-3 rounded-lg border bg-card text-card-foreground shadow-sm transition-all hover:shadow-md"
          :class="{'opacity-60': !mod.enabled}"
        >
          <div class="w-12 h-12 rounded bg-muted flex items-center justify-center overflow-hidden flex-shrink-0 border">
            <img v-if="mod.iconUrl" :src="convertFileSrc(mod.iconUrl)" class="w-full h-full object-cover" />
            <Puzzle v-else class="w-6 h-6 text-muted-foreground" />
          </div>
          
          <div class="flex-1 min-w-0">
            <div class="font-medium text-sm truncate" :title="mod.name || mod.filename">{{ mod.name || mod.filename }}</div>
            <div class="text-xs text-muted-foreground mt-1 flex items-center gap-2">
              <span v-if="mod.version" class="bg-secondary px-1.5 py-0.5 rounded">{{ mod.version }}</span>
              <span class="truncate">{{ mod.filename }}</span>
            </div>
          </div>
          
          <div class="flex items-center gap-2 flex-shrink-0">
            <DButton 
              :variant="mod.enabled ? 'outline' : 'secondary'"
              size="sm"
              @click="toggleMod(mod)"
              :disabled="mod.isToggling"
              class="w-20"
            >
              <template v-if="mod.isToggling">
                <div class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin"></div>
              </template>
              <template v-else>
                {{ mod.enabled ? $t('instances.disable', 'Disable') : $t('instances.enable', 'Enable') }}
              </template>
            </DButton>
            
            <DButton 
              variant="danger" 
              size="sm" 
              class="px-2"
              @click="confirmDelete(mod)"
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
        <AlertDialogTitle>{{ $t('instances.deleteModConfirm', 'Are you sure you want to delete this mod?') }}</AlertDialogTitle>
        <AlertDialogDescription>
          {{ $t('instances.deleteModConfirmDesc', 'This action cannot be undone. The mod file will be permanently deleted.') }}
          <br/><br/>
          <strong class="text-foreground">{{ modToDelete?.name || modToDelete?.filename }}</strong>
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
import { useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { Package, Search, Puzzle, Download, Trash2 } from '@lucide/vue';
import { AlertDialog, AlertDialogTitle, AlertDialogDescription } from '../ui/alert-dialog';
import DInput from '../ui/DInput.vue';
import DButton from '../ui/DButton.vue';
import { toast } from '../../composables/useToast';
import { getErrorMessage } from '../../utils/error';

const props = defineProps<{
  instanceId: string;
}>();

const router = useRouter();
const { t } = useI18n();

interface LocalMod {
  filename: string;
  enabled: boolean;
  size: number;
  modId?: string;
  name?: string;
  version?: string;
  iconUrl?: string;
  isToggling?: boolean;
}

const mods = ref<LocalMod[]>([]);
const loading = ref(false);
const searchQuery = ref('');

const showDeleteConfirm = ref(false);
const modToDelete = ref<LocalMod | null>(null);
const isDeleting = ref(false);

const filteredMods = computed(() => {
  if (!searchQuery.value) return mods.value;
  const q = searchQuery.value.toLowerCase();
  return mods.value.filter(mod => 
    (mod.name && mod.name.toLowerCase().includes(q)) || 
    mod.filename.toLowerCase().includes(q)
  );
});

async function loadMods() {
  if (!props.instanceId) return;
  
  loading.value = true;
  try {
    const result = await invoke<any[]>('get_installed_mods', { versionId: props.instanceId });
    mods.value = result.map(mod => ({
      ...mod,
      isToggling: false
    }));
  } catch (error) {
    console.error('Failed to load mods:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    loading.value = false;
  }
}

watch(() => props.instanceId, loadMods, { immediate: true });

async function toggleMod(mod: LocalMod) {
  if (mod.isToggling) return;
  
  mod.isToggling = true;
  const targetState = !mod.enabled;
  
  try {
    await invoke('toggle_mod_status', {
      versionId: props.instanceId,
      filename: mod.filename,
      enable: targetState
    });
    mod.enabled = targetState;
  } catch (error) {
    console.error('Failed to toggle mod:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    mod.isToggling = false;
  }
}

function confirmDelete(mod: LocalMod) {
  modToDelete.value = mod;
  showDeleteConfirm.value = true;
}

async function executeDelete() {
  if (!modToDelete.value) return;
  
  isDeleting.value = true;
  try {
    await invoke('delete_local_mod', {
      versionId: props.instanceId,
      filename: modToDelete.value.filename,
      isEnabled: modToDelete.value.enabled
    });
    
    // Remove from list
    mods.value = mods.value.filter(m => m.filename !== modToDelete.value!.filename);
    showDeleteConfirm.value = false;
  } catch (error) {
    console.error('Failed to delete mod:', error);
    toast.error(
      t('common.error', 'Error'),
      getErrorMessage(error)
    );
  } finally {
    isDeleting.value = false;
  }
}

function handleDownloadMore() {
  router.push({ path: '/downloads', query: { tab: 'mod', instanceId: props.instanceId } });
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
