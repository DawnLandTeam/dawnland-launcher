<script setup lang="ts">
import { ref } from 'vue';
import DInput from '../components/ui/DInput.vue';
import DButton from '../components/ui/DButton.vue';
import DToggleGroup from '../components/ui/DToggleGroup.vue';
import DSelect from '../components/ui/DSelect.vue';
import DMultiSelect from '../components/ui/DMultiSelect.vue';
import DSidebarTabs from '../components/ui/DSidebarTabs.vue';
import { Plus, Star, Heart, Leaf, Box, Package } from '@lucide/vue';

// State
const inputValue = ref('');
const toggleValue = ref('online');
const singleSelectValue = ref('');
const multiSelectValue = ref<string[]>([]);
const sidebarTabValue = ref('tab1');

// Data
const toggleOptions = [
  { label: '在线搜索', value: 'online' },
  { label: '本地上传', value: 'local' }
];

const sidebarTabs = [
  { id: 'tab1', name: 'Instances', icon: Box, group: 'Library' },
  { id: 'tab2', name: 'Modpacks', icon: Package, group: 'Library' },
  { id: 'tab3', name: 'Store (Coming soon)', icon: Star, group: 'Discovery', disabled: true },
  { id: 'tab4', name: 'Community', icon: Heart, group: 'Discovery' },
];

const selectOptions = [
  { label: 'Apple', value: 'apple', group: 'Fruits', icon: Heart },
  { label: 'Banana', value: 'banana', group: 'Fruits', icon: Star },
  { label: 'Carrot', value: 'carrot', group: 'Vegetables', icon: Leaf },
  { label: 'Potato (Out of stock)', value: 'potato', group: 'Vegetables', disabled: true },
  { label: 'Beef', value: 'beef', group: 'Meat' },
];

const handleCustomAdd = () => {
  alert('Clicked: Add New Option');
};
</script>

<template>
  <div class="p-8 h-full overflow-auto bg-neutral-50 dark:bg-zinc-950 text-neutral-900 dark:text-zinc-100">
    <div class="max-w-3xl mx-auto space-y-12">
      
      <div>
        <h1 class="text-3xl font-bold mb-2">Dawnland UI Components</h1>
        <p class="text-neutral-500">A playground to test the unified design system components.</p>
      </div>

      <!-- Text Input -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">1. Text Input (DInput)</h2>
        <div class="grid grid-cols-2 gap-8">
          <div class="space-y-2">
            <label class="text-sm font-medium">Default</label>
            <DInput v-model="inputValue" placeholder="Search keywords..." />
            <p class="text-xs text-neutral-500">Value: {{ inputValue || 'empty' }}</p>
          </div>
          <div class="space-y-2">
            <label class="text-sm font-medium">Disabled</label>
            <DInput disabled placeholder="Cannot type here..." />
          </div>
        </div>
      </section>

      <!-- Button -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">2. Button (DButton)</h2>
        <div class="flex items-center gap-4">
          <DButton>Primary Action</DButton>
          <DButton disabled>Disabled Action</DButton>
        </div>
      </section>

      <!-- Toggle Group -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">3. Toggle Group (DToggleGroup)</h2>
        <div class="space-y-2">
          <DToggleGroup :options="toggleOptions" v-model="toggleValue" />
          <p class="text-xs text-neutral-500 mt-2">Selected: {{ toggleValue }}</p>
        </div>
      </section>

      <!-- Sidebar Tabs -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">4. Sidebar Tabs (DSidebarTabs)</h2>
        <div class="flex gap-8 items-start">
          <DSidebarTabs title="Downloads" :tabs="sidebarTabs" v-model="sidebarTabValue" />
          <div class="flex-1 p-4 rounded-xl border border-dashed border-neutral-300 dark:border-zinc-700 h-full flex items-center justify-center text-neutral-500">
            Current Tab: {{ sidebarTabValue }}
          </div>
        </div>
      </section>

      <!-- Single Select -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">5. Single Select (DSelect)</h2>
        <div class="grid grid-cols-2 gap-8">
          <div class="space-y-2">
            <label class="text-sm font-medium">With Groups & Disabled Items</label>
            <DSelect 
              v-model="singleSelectValue" 
              :options="selectOptions" 
              placeholder="Select an ingredient..." 
            >
              <template #append>
                <div class="px-1 py-1 mt-1 border-t border-neutral-100 dark:border-zinc-800">
                  <button 
                    @click="handleCustomAdd"
                    class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-sm text-emerald-600 hover:bg-emerald-50 dark:hover:bg-emerald-950/30 transition-colors"
                  >
                    <Plus class="h-4 w-4" />
                    <span>Add new option...</span>
                  </button>
                </div>
              </template>
            </DSelect>
            <p class="text-xs text-neutral-500">Value: {{ singleSelectValue || 'none' }}</p>
          </div>
          
          <div class="space-y-2">
            <label class="text-sm font-medium">Disabled Select</label>
            <DSelect disabled :options="selectOptions" placeholder="Select disabled..." />
          </div>
        </div>
      </section>

      <!-- Multi Select -->
      <section class="space-y-4 border p-6 rounded-xl border-neutral-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 shadow-sm">
        <h2 class="text-xl font-semibold border-b pb-2 dark:border-zinc-800">6. Multi Select (DMultiSelect)</h2>
        <div class="grid grid-cols-2 gap-8">
          <div class="space-y-2">
            <label class="text-sm font-medium">With Search & Checkmarks</label>
            <DMultiSelect 
              v-model="multiSelectValue" 
              :options="selectOptions" 
              placeholder="Select multiple ingredients..." 
            />
            <p class="text-xs text-neutral-500 break-words">Values: [{{ multiSelectValue.join(', ') }}]</p>
          </div>
        </div>
      </section>

    </div>
  </div>
</template>
