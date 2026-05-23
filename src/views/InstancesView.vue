<script setup lang="ts">
import { ref } from "vue";
import { Gamepad2, Plus, Package } from "@lucide/vue";
import InstallInstanceModal from "../components/InstallInstanceModal.vue";

// State
const showInstallModal = ref(false);

// Placeholder — will be loaded from Rust backend later
interface InstalledInstance {
  id: string;
  name: string;
  version: string;
  lastPlayed?: string;
}

const installedInstances = ref<InstalledInstance[]>([]);
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- Empty State -->
    <div
      v-if="installedInstances.length === 0"
      class="flex flex-1 flex-col items-center justify-center gap-4 p-6"
    >
      <div
        class="flex h-20 w-20 items-center justify-center rounded-2xl bg-muted"
      >
        <Package class="h-10 w-10 text-muted-foreground" />
      </div>
      <div class="text-center space-y-1">
        <h2 class="text-xl font-semibold">No instances yet</h2>
        <p class="text-sm text-muted-foreground">
          Install a Minecraft version to get started.
        </p>
      </div>
      <button
        @click="showInstallModal = true"
        class="flex items-center gap-2 rounded-md bg-primary px-5 py-2.5 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
      >
        <Plus class="h-4 w-4" />
        Install Instance
      </button>
    </div>

    <!-- List State -->
    <div v-else class="flex flex-1 flex-col p-6 space-y-6">
      <!-- Header -->
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3">
          <Gamepad2 class="h-7 w-7 text-primary" />
          <div>
            <h1 class="text-2xl font-bold">Game Instances</h1>
            <p class="text-sm text-muted-foreground">
              {{ installedInstances.length }} instance{{ installedInstances.length !== 1 ? 's' : '' }} installed
            </p>
          </div>
        </div>
        <button
          @click="showInstallModal = true"
          class="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 transition-colors"
        >
          <Plus class="h-4 w-4" />
          New Instance
        </button>
      </div>

      <!-- Instance Grid -->
      <div class="grid grid-cols-3 gap-4">
        <div
          v-for="instance in installedInstances"
          :key="instance.id"
          class="group rounded-lg border bg-card p-4 hover:border-primary/50 transition-colors cursor-pointer"
        >
          <div class="flex items-start justify-between">
            <div class="space-y-1">
              <h3 class="font-medium">{{ instance.name }}</h3>
              <p class="text-xs text-muted-foreground">
                {{ instance.version }}
              </p>
            </div>
            <Gamepad2 class="h-5 w-5 text-muted-foreground group-hover:text-primary transition-colors" />
          </div>
          <p
            v-if="instance.lastPlayed"
            class="mt-2 text-xs text-muted-foreground"
          >
            Last played: {{ instance.lastPlayed }}
          </p>
        </div>
      </div>
    </div>

    <!-- Install Instance Modal -->
    <InstallInstanceModal v-model:open="showInstallModal" />
  </div>
</template>
