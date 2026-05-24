<script setup lang="ts">
import { ref, computed } from "vue";
import { X, Copy, AlertTriangle } from "@lucide/vue";

const props = defineProps<{
  open: boolean;
  exitCode: number;
  versionId: string;
  logs: string[];
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

const copied = ref(false);

// Get last 50 lines of logs
const crashLogs = computed(() => {
  const lines = props.logs || [];
  return lines.slice(-50);
});

const formattedLogs = computed(() => {
  return crashLogs.value.join("\n");
});

function close() {
  emit("update:open", false);
}

async function copyLogs() {
  try {
    await navigator.clipboard.writeText(formattedLogs.value);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (e) {
    console.error("Failed to copy logs:", e);
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
      <div
        v-if="open"
        class="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
      >
        <!-- Frosted glass backdrop -->
        <div
          class="absolute inset-0 bg-black/40 backdrop-blur-sm pointer-events-auto"
          @click="close"
        />

        <!-- Crash Report Content -->
        <div
          class="relative z-10 w-full max-w-3xl gap-4 border-2 border-red-500 bg-white dark:bg-zinc-900 p-6 shadow-2xl rounded-lg max-h-[85vh] overflow-hidden flex flex-col pointer-events-auto"
        >
          <!-- Header with red accent -->
          <div class="flex items-center justify-between pb-4 border-b-2 border-red-500 -mx-6 px-6 -mt-6 pt-6 bg-red-50 dark:bg-red-950/30">
            <div class="flex items-center gap-3">
              <AlertTriangle class="h-6 w-6 text-red-600" />
              <div>
                <h3 class="font-bold text-lg text-red-600">Game Crashed!</h3>
                <p class="text-sm text-muted-foreground">
                  {{ versionId }} · Exit Code: {{ exitCode }}
                </p>
              </div>
            </div>
            <button
              @click="close"
              class="text-muted-foreground hover:text-foreground"
            >
              <X class="h-5 w-5" />
            </button>
          </div>

          <!-- Crash Log Display -->
          <div class="flex-1 mt-4 overflow-hidden flex flex-col min-h-0">
            <div class="flex items-center justify-between mb-2">
              <label class="text-sm font-medium">Crash Log (Last 50 lines)</label>
              <button
                @click="copyLogs"
                class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium border rounded-md hover:bg-muted transition-colors"
              >
                <Copy class="h-3.5 w-3.5" />
                {{ copied ? "Copied!" : "Copy Logs" }}
              </button>
            </div>
            <textarea
              readonly
              :value="formattedLogs"
              class="flex-1 w-full px-4 py-3 font-mono text-xs bg-black text-green-400 rounded-lg resize-none border-0 focus:ring-0"
              style="min-height: 300px;"
            />
          </div>

          <!-- Footer -->
          <div class="flex justify-end gap-2 mt-4 pt-4 border-t">
            <button
              @click="close"
              class="px-4 py-2 text-sm font-medium bg-primary text-primary-foreground rounded-md hover:bg-primary/90 transition-colors"
            >
              Close
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-enter-active,
.dialog-leave-active {
  transition: opacity 150ms ease;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-active .relative,
.dialog-leave-active .relative {
  transition: transform 150ms ease, opacity 150ms ease;
}

.dialog-enter-from .relative {
  transform: scale(0.95);
  opacity: 0;
}

.dialog-leave-to .relative {
  transform: scale(0.95);
  opacity: 0;
}
</style>