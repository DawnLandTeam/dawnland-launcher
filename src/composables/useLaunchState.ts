import { ref } from 'vue';

// Global singleton states for instances
export const launchingInstances = ref<Set<string>>(new Set());
export const jvmSpawnedInstances = ref<Set<string>>(new Set());
export const runningInstances = ref<Set<string>>(new Set());
export const repairingInstances = ref<Set<string>>(new Set());
