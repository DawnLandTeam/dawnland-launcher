import { ref } from 'vue';

export const isAppBusy = ref(false);

export function setAppBusy(busy: boolean) {
  isAppBusy.value = busy;
}
