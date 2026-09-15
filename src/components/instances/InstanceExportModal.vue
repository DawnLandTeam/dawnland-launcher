<script setup lang="ts">
import { ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Check, X, FileArchive, Package, Save, Link2, Search, ArrowRight } from '@lucide/vue';
import { save } from '@tauri-apps/plugin-dialog';
import DButton from '../ui/DButton.vue';
import DInput from '../ui/DInput.vue';
import { DialogContent, DialogTitle, DialogDescription } from '../ui/dialog';

const props = defineProps<{
  open: boolean;
  instanceId: string;
}>();

const emit = defineEmits(['update:open']);

const form = ref({
  name: '',
  version: '',
  author: '',
  format: 'modrinth', // 'modrinth' | 'curseforge'
  includeSaves: false,
});

watch(() => props.open, async (newVal) => {
  if (newVal) {
    currentState.value = 'IDLE';
    exportError.value = '';
    matchedMods.value = [];
    unmatchedMods.value = [];
    isAutoResolvingAll.value = false;
    
    try {
      const details = await invoke<any>('get_instance_details', { versionId: props.instanceId });
      form.value.name = details.name || '';
      form.value.version = details.modpackVersion || '';
      form.value.author = '';
    } catch (e) {
      console.warn("Failed to load instance details for export defaults:", e);
    }
  }
});

type StepState = 'IDLE' | 'ANALYZING' | 'REVIEW' | 'EXPORTING' | 'SUCCESS';
const currentState = ref<StepState>('IDLE');

const exportProgress = ref({ step: '', translationKey: '', current: null as number | null, totalItems: null as number | null, progress: 0, total: 100 });
const exportError = ref('');

// Analysis Data
const matchedMods = ref<any[]>([]);
const unmatchedMods = ref<any[]>([]);

// Manual Match Data
const resolveInputs = ref<Record<string, string>>({});
const resolving = ref<Record<string, boolean>>({});
const resolveErrors = ref<Record<string, string>>({});
const manualChoices = ref<Record<string, any[]>>({});

async function startAnalysis() {
  if (currentState.value !== 'IDLE') return;
  exportError.value = '';
  currentState.value = 'ANALYZING';
  exportProgress.value = { step: 'Starting analysis...', translationKey: 'instances.export.progress.starting', current: null, totalItems: null, progress: 0, total: 100 };

  const unlisten = await listen('export-progress', (event: any) => {
    exportProgress.value = event.payload;
  });

  try {
    const analysis: any = await invoke('analyze_export_instance', {
      versionId: props.instanceId,
      format: form.value.format,
    });
    
    matchedMods.value = analysis.matchedMods || [];
    unmatchedMods.value = analysis.unmatchedMods || [];
    
    // Pre-fill resolve inputs with name or modId if available
    resolveInputs.value = {};
    unmatchedMods.value.forEach(mod => {
      resolveInputs.value[mod.filename] = mod.name || mod.modId || '';
    });
    
    if (unmatchedMods.value.length === 0) {
      // If all matched perfectly, just go to packaging directly
      await triggerPackage();
    } else {
      // Allow user to review and manually match
      currentState.value = 'REVIEW';
    }
  } catch (err: any) {
    exportError.value = typeof err === 'string' ? err : err.message;
    currentState.value = 'IDLE';
  } finally {
    unlisten();
  }
}

async function resolveManualMatch(mod: any) {
  const projectId = resolveInputs.value[mod.filename];
  if (!projectId) return;

  resolving.value[mod.filename] = true;
  resolveErrors.value[mod.filename] = '';
  delete manualChoices.value[mod.filename];

  try {
    const response: any = await invoke('resolve_manual_match', {
      versionId: props.instanceId,
      format: form.value.format,
      filename: mod.filename,
      projectId: projectId.trim(),
    });

    if (response.status === 'Matched' || response.Matched) {
      // Backend might return { Matched: { matched_mod: ... } } or { status: 'Matched', matched_mod: ... }
      const newMatch = response.matched_mod || response.matchedMod || (response.Matched ? response.Matched.matched_mod : null);
      if (newMatch) {
        unmatchedMods.value = unmatchedMods.value.filter(m => m.filename !== mod.filename);
        matchedMods.value.push(newMatch);
      }
    } else if (response.status === 'Choices' || response.Choices) {
      const choices = response.choices || (response.Choices ? response.Choices.choices : []);
      manualChoices.value[mod.filename] = choices;
    }
  } catch (err: any) {
    resolveErrors.value[mod.filename] = typeof err === 'string' ? err : err.message;
  } finally {
    resolving.value[mod.filename] = false;
  }
}

const isAutoResolvingAll = ref(false);

async function autoResolveAll() {
  if (isAutoResolvingAll.value || unmatchedMods.value.length === 0) return;
  isAutoResolvingAll.value = true;
  
  // Clone to avoid mutation issues during iteration
  const modsToResolve = [...unmatchedMods.value];
  
  for (const mod of modsToResolve) {
    if (!isAutoResolvingAll.value) break; // User stopped it? (Optional, currently no stop button, but good to have)
    if (!unmatchedMods.value.find(m => m.filename === mod.filename)) continue;
    
    // Auto-Scroll to the item
    const elId = `unmatched-${mod.filename.replace(/[^a-zA-Z0-9]/g, '_')}`;
    const el = document.getElementById(elId);
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'center' });

    await resolveManualMatch(mod);

    // If it requires manual choice, PAUSE the loop until the choice is resolved
    if (manualChoices.value[mod.filename]) {
      await new Promise<void>(resolve => {
        const unwatch = watch(() => manualChoices.value[mod.filename], (newVal) => {
          if (!newVal) { // Choice made or cancelled
            unwatch();
            resolve();
          }
        });
        
        const unwatchCancel = watch(isAutoResolvingAll, (val) => {
          if (!val) { // User manually toggled off auto-resolve
            unwatch();
            unwatchCancel();
            resolve();
          }
        });
      });
    }
  }
  
  isAutoResolvingAll.value = false;
}

function applyChoice(mod: any, choice: any) {
  unmatchedMods.value = unmatchedMods.value.filter(m => m.filename !== mod.filename);
  matchedMods.value.push(choice.matched_mod || choice.matchedMod);
  delete manualChoices.value[mod.filename];
}

async function triggerPackage() {
  const filters = form.value.format === 'modrinth' 
    ? [{ name: 'Modrinth Modpack', extensions: ['mrpack'] }]
    : [{ name: 'CurseForge Modpack', extensions: ['zip'] }];

  const outputPath = await save({
    filters,
    defaultPath: `${form.value.name.replace(/[^a-z0-9]/gi, '_').toLowerCase()}-${form.value.version}.${form.value.format === 'modrinth' ? 'mrpack' : 'zip'}`
  });

  if (!outputPath) return; // user cancelled, stay on current state

  currentState.value = 'EXPORTING';
  exportProgress.value = { step: 'Zipping archive...', translationKey: 'instances.export.progress.zipping', current: null, totalItems: null, progress: 70, total: 100 };

  const unlisten = await listen('export-progress', (event: any) => {
    exportProgress.value = event.payload;
  });

  try {
    await invoke('build_export_instance', {
      request: {
        versionId: props.instanceId,
        name: form.value.name,
        version: form.value.version,
        author: form.value.author,
        format: form.value.format,
        includeSaves: form.value.includeSaves,
        outputPath
      },
      analysisResult: {
        matchedMods: matchedMods.value,
        unmatchedMods: unmatchedMods.value,
      }
    });
    currentState.value = 'SUCCESS';
  } catch (err: any) {
    exportError.value = typeof err === 'string' ? err : err.message;
    // Go back to Review if it fails during package
    currentState.value = matchedMods.value.length || unmatchedMods.value.length ? 'REVIEW' : 'IDLE';
  } finally {
    unlisten();
  }
}

function handleClose() {
  if (currentState.value === 'ANALYZING' || currentState.value === 'EXPORTING') return;
  emit('update:open', false);
  setTimeout(() => {
    currentState.value = 'IDLE';
    exportError.value = '';
    matchedMods.value = [];
    unmatchedMods.value = [];
    resolveInputs.value = {};
    resolving.value = {};
    resolveErrors.value = {};
    manualChoices.value = {};
  }, 300);
}

function formatBytes(bytes: number, decimals = 2) {
    if (!+bytes) return '0 Bytes'
    const k = 1024
    const dm = decimals < 0 ? 0 : decimals
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`
}
</script>

<template>
  <DialogContent :open="open" @update:open="handleClose" class="sm:max-w-[550px] max-h-[90vh]">
    <div class="flex flex-col space-y-1.5 text-center sm:text-left mb-4">
      <DialogTitle>{{ $t('instances.export.title', 'Export Modpack') }}</DialogTitle>
      <DialogDescription v-if="currentState === 'IDLE'">
        {{ $t('instances.export.description', 'Package this instance into a standard format for sharing.') }}
      </DialogDescription>
      <DialogDescription v-else-if="currentState === 'REVIEW'">
        {{ unmatchedMods.length }} {{ $t('instances.export.reviewModsDesc', 'mods could not be matched automatically. You can manually link them below or just continue to include them directly.') }}
      </DialogDescription>
    </div>

    <!-- IDLE (Config) State -->
    <div v-if="currentState === 'IDLE'" class="grid gap-4 py-4">
      <div class="grid gap-2">
        <label class="text-sm font-medium">{{ $t('instances.export.name', 'Modpack Name') }}</label>
        <DInput v-model="form.name" />
      </div>
      
      <div class="grid grid-cols-2 gap-4">
        <div class="grid gap-2">
          <label class="text-sm font-medium">{{ $t('instances.export.version', 'Version') }}</label>
          <DInput v-model="form.version" />
        </div>
        <div class="grid gap-2">
          <label class="text-sm font-medium">{{ $t('instances.export.author', 'Author') }}</label>
          <DInput v-model="form.author" />
        </div>
      </div>

      <div class="grid gap-2 mt-2">
        <label class="text-sm font-medium">{{ $t('instances.export.format', 'Format') }}</label>
        <div class="flex gap-2">
          <div 
            @click="form.format = 'modrinth'"
            :class="[
              'flex-1 flex flex-col items-center p-3 rounded-lg border-2 cursor-pointer transition-all',
              form.format === 'modrinth' 
                ? 'border-emerald-500 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400' 
                : 'border-zinc-200 dark:border-zinc-800 hover:border-emerald-500/50 hover:bg-emerald-500/5'
            ]"
          >
            <Package class="w-6 h-6 mb-2" />
            <span class="text-sm font-bold">Modrinth</span>
            <span class="text-xs opacity-70">.mrpack</span>
          </div>
          
          <div 
            @click="form.format = 'curseforge'"
            :class="[
              'flex-1 flex flex-col items-center p-3 rounded-lg border-2 cursor-pointer transition-all',
              form.format === 'curseforge' 
                ? 'border-orange-500 bg-orange-500/10 text-orange-600 dark:text-orange-400' 
                : 'border-zinc-200 dark:border-zinc-800 hover:border-orange-500/50 hover:bg-orange-500/5'
            ]"
          >
            <FileArchive class="w-6 h-6 mb-2" />
            <span class="text-sm font-bold">CurseForge</span>
            <span class="text-xs opacity-70">.zip</span>
          </div>
        </div>
      </div>
      
      <div class="flex items-center space-x-2 mt-2">
        <input type="checkbox" id="includeSaves" v-model="form.includeSaves" class="rounded border-zinc-300 text-emerald-600 focus:ring-emerald-600" />
        <label for="includeSaves" class="text-sm text-zinc-600 dark:text-zinc-400 cursor-pointer">
          {{ $t('instances.export.includeSaves', 'Include Save Worlds (saves folder)') }}
        </label>
      </div>
      
      <div v-if="exportError" class="p-3 bg-red-500/10 text-red-500 text-sm rounded-md flex items-start mt-2">
        <X class="w-4 h-4 mr-2 mt-0.5 shrink-0" />
        <span>{{ exportError }}</span>
      </div>
    </div>

    <!-- REVIEW (Manual Match) State -->
    <div v-else-if="currentState === 'REVIEW'" class="flex flex-col gap-2 py-2">
      <div v-if="unmatchedMods.length > 0" class="flex justify-end mb-1">
        <DButton variant="secondary" class="h-7 text-xs px-2" @click="autoResolveAll" :disabled="isAutoResolvingAll">
          <Search class="w-3 h-3 mr-1.5" :class="{ 'animate-pulse': isAutoResolvingAll }" />
          {{ isAutoResolvingAll ? $t('common.processing', 'Processing...') : $t('instances.export.autoResolve', 'Auto-Resolve All') }}
        </DButton>
      </div>
      
      <div class="bg-white dark:bg-zinc-900 rounded-md border border-zinc-200 dark:border-zinc-800 overflow-hidden max-h-[350px] overflow-y-auto">
        <div v-if="unmatchedMods.length === 0" class="flex flex-col items-center justify-center py-6 text-zinc-500">
          <Check class="w-10 h-10 text-emerald-500 mb-2" />
          <p>{{ $t('instances.export.allMatched', 'All mods matched perfectly!') }}</p>
        </div>
        <div v-else class="divide-y divide-zinc-100 dark:divide-zinc-800">
          <div v-for="mod in unmatchedMods" :key="mod.filename" :id="'unmatched-' + mod.filename.replace(/[^a-zA-Z0-9]/g, '_')" class="p-2 flex flex-col hover:bg-zinc-50 dark:hover:bg-zinc-800/50 transition-colors">
            <div class="flex items-center justify-between gap-3">
              <div class="flex flex-col min-w-0 flex-1">
                <div class="flex items-center gap-1.5 overflow-hidden">
                  <span class="text-sm font-medium truncate text-zinc-900 dark:text-zinc-100" :title="mod.name || mod.filename">{{ mod.name || mod.filename }}</span>
                  <span v-if="mod.version" class="text-[10px] px-1.5 py-0.5 rounded-sm bg-zinc-100 dark:bg-zinc-800 text-zinc-500 whitespace-nowrap shrink-0">{{ mod.version }}</span>
                </div>
                <div class="flex items-center gap-1.5 text-[10px] text-zinc-400 mt-0.5">
                  <span class="truncate" :title="mod.filename">{{ mod.filename }}</span>
                  <span class="shrink-0 text-zinc-300 dark:text-zinc-600">|</span>
                  <span class="shrink-0">{{ formatBytes(mod.size) }}</span>
                </div>
              </div>
              
              <div class="flex items-center gap-1.5 shrink-0 w-[180px]">
                <DInput 
                  v-model="resolveInputs[mod.filename]" 
                  class="h-7 text-xs px-2 flex-1" 
                  :placeholder="form.format === 'modrinth' ? $t('instances.export.slugPlaceholder', 'Project Slug (e.g. sodium)') : $t('instances.export.idPlaceholder', 'Project ID (e.g. 394468)')" 
                  @keyup.enter="resolveManualMatch(mod)"
                />
                <DButton size="sm" variant="outline" class="h-7 px-2.5 text-xs shrink-0" :disabled="!resolveInputs[mod.filename] || resolving[mod.filename]" @click="resolveManualMatch(mod)">
                  <Link2 v-if="!resolving[mod.filename]" class="w-3 h-3 mr-1" />
                  <div v-else class="w-3 h-3 mr-1 border-2 border-zinc-300 border-t-zinc-600 rounded-full animate-spin"></div>
                  {{ $t('common.link', 'Link') }}
                </DButton>
              </div>
            </div>
            <div v-if="resolveErrors[mod.filename]" class="text-[11px] text-red-500 mt-1 px-1">{{ $te(resolveErrors[mod.filename]) ? $t(resolveErrors[mod.filename]) : resolveErrors[mod.filename] }}</div>
            
            <!-- Choices Dropdown -->
            <div v-if="manualChoices[mod.filename] && manualChoices[mod.filename].length > 0" class="mt-2 flex flex-col gap-1.5 p-1">
              <div class="text-[11px] text-zinc-500 font-medium px-1 flex justify-between items-center">
                <span>{{ $t('instances.export.selectVersion', 'Select a compatible version:') }}</span>
                <button class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300" @click="delete manualChoices[mod.filename]">
                  <X class="w-3.5 h-3.5" />
                </button>
              </div>
              <div class="max-h-[140px] overflow-y-auto rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900/50 flex flex-col divide-y divide-zinc-200 dark:divide-zinc-700/50">
                <div v-for="choice in manualChoices[mod.filename]" :key="choice.version_id || choice.versionId" 
                     class="p-2 text-xs hover:bg-zinc-100 dark:hover:bg-zinc-800 cursor-pointer flex justify-between items-center group transition-colors"
                     @click="applyChoice(mod, choice)">
                  <div class="flex flex-col min-w-0 flex-1">
                    <span class="font-medium text-zinc-900 dark:text-zinc-100 truncate" :title="choice.version_name || choice.versionName">{{ choice.version_name || choice.versionName }}</span>
                    <span class="text-[10px] text-zinc-500 truncate mt-0.5">{{ (choice.game_versions || choice.gameVersions || []).join(', ') }}<span v-if="(choice.loaders || []).length > 0"> | {{ (choice.loaders || []).join(', ') }}</span></span>
                  </div>
                  <div class="w-6 h-6 rounded-full bg-emerald-500/10 flex items-center justify-center opacity-0 group-hover:opacity-100 shrink-0 ml-2 transition-opacity">
                    <Check class="w-3.5 h-3.5 text-emerald-500" />
                  </div>
                </div>
              </div>
            </div>
            
          </div>
        </div>
      </div>
      
      <div v-if="exportError" class="p-3 bg-red-500/10 text-red-500 text-sm rounded-md flex items-start mt-2">
        <X class="w-4 h-4 mr-2 mt-0.5 shrink-0" />
        <span>{{ exportError }}</span>
      </div>
    </div>

    <!-- Analyzing / Exporting Progress State -->
    <div v-else-if="currentState === 'ANALYZING' || currentState === 'EXPORTING'" class="py-8 flex flex-col items-center justify-center space-y-4">
      <div class="w-12 h-12 border-4 border-zinc-200 border-t-emerald-500 rounded-full animate-spin"></div>
      <div class="text-center space-y-1 w-full px-4">
        <p class="font-medium text-zinc-900 dark:text-zinc-100">
          {{ exportProgress.translationKey ? $t(exportProgress.translationKey, { current: exportProgress.current || 0, total: exportProgress.totalItems || 0 }) : exportProgress.step }}
        </p>
        <div class="w-full bg-zinc-100 dark:bg-zinc-800 rounded-full h-2 overflow-hidden">
          <div 
            class="bg-emerald-500 h-full transition-all duration-300"
            :style="{ width: `${exportProgress.progress}%` }"
          ></div>
        </div>
        <p class="text-xs text-zinc-500">{{ exportProgress.progress }}%</p>
      </div>
    </div>

    <!-- Success State -->
    <div v-else-if="currentState === 'SUCCESS'" class="py-8 flex flex-col items-center justify-center space-y-4">
      <div class="w-16 h-16 bg-emerald-500/10 text-emerald-500 rounded-full flex items-center justify-center">
        <Check class="w-8 h-8" />
      </div>
      <div class="text-center">
        <p class="font-medium text-lg">{{ $t('instances.export.success', 'Export Successful') }}</p>
        <p class="text-sm text-zinc-500 mt-1">{{ $t('instances.export.successDesc', 'The modpack has been saved.') }}</p>
      </div>
    </div>

    <!-- Footer Buttons -->
    <div v-if="currentState !== 'ANALYZING' && currentState !== 'EXPORTING'" class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2 mt-4 pt-4 border-t border-zinc-100 dark:border-zinc-800">
      <DButton v-if="currentState !== 'SUCCESS'" variant="outline" @click="handleClose">
        {{ $t('common.cancel', 'Cancel') }}
      </DButton>
      
      <DButton v-if="currentState === 'IDLE'" variant="primary" :disabled="!form.name || !form.version || !form.author" @click="startAnalysis">
        {{ $t('common.next', 'Next') }}
        <ArrowRight class="w-4 h-4 ml-2" />
      </DButton>

      <DButton v-if="currentState === 'REVIEW'" variant="primary" @click="triggerPackage">
        <Save class="w-4 h-4 mr-2" />
        {{ $t('instances.export.start', 'Export Modpack') }}
      </DButton>

      <DButton v-if="currentState === 'SUCCESS'" variant="primary" @click="handleClose" class="w-full sm:w-auto">
        {{ $t('common.close', 'Close') }}
      </DButton>
    </div>
  </DialogContent>
</template>

