import { ref } from 'vue';

export interface ToastMessage {
  id: number;
  title: string;
  description?: string;
  type?: 'success' | 'error' | 'info';
}

const toasts = ref<ToastMessage[]>([]);
let nextId = 0;

export const useToast = () => {
  const addToast = (toast: Omit<ToastMessage, 'id'>) => {
    const id = nextId++;
    toasts.value.push({ ...toast, id });
    setTimeout(() => {
      removeToast(id);
    }, 4000);
  };

  const removeToast = (id: number) => {
    const index = toasts.value.findIndex((t) => t.id === id);
    if (index !== -1) {
      toasts.value.splice(index, 1);
    }
  };

  const success = (title: string, description?: string) => {
    addToast({ title, description, type: 'success' });
  };

  const error = (title: string, description?: string) => {
    addToast({ title, description, type: 'error' });
  };

  const info = (title: string, description?: string) => {
    addToast({ title, description, type: 'info' });
  };

  return {
    toasts,
    success,
    error,
    info,
    removeToast,
  };
};

// Global instance to be used by non-setup functions or directly imported
export const toast = useToast();
