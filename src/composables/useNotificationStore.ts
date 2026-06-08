import { ref, computed } from 'vue';

export interface NotificationMessage {
  id: string | number;
  title: string;
  description?: string;
  type?: 'success' | 'error' | 'info';
  status: 'unread' | 'read';
  isPopup: boolean;
  duration: number; // in ms
  timestamp: number;
}

// Global state
const notifications = ref<NotificationMessage[]>([]);
const isCenterOpen = ref(false);
let nextId = 0;

// Track active popup timeouts to clear them if updated
const popupTimeouts = new Map<string | number, number>();

export const useNotificationStore = () => {
  const unreadCount = computed(() => notifications.value.filter(n => n.status === 'unread').length);

  const toggleCenter = () => {
    isCenterOpen.value = !isCenterOpen.value;
    if (isCenterOpen.value) {
      // Mark all as read when opening
      markAllAsRead();
    }
  };

  const addNotification = (payload: Omit<NotificationMessage, 'timestamp' | 'status' | 'id'> & { id?: string | number, status?: 'unread' | 'read' }) => {
    const id = payload.id !== undefined ? payload.id : `notif_${nextId++}`;
    
    // Check if it already exists (to prevent duplicates if manually using add instead of update)
    const existingIndex = notifications.value.findIndex(n => n.id === id);
    
    const newNotif: NotificationMessage = {
      id,
      title: payload.title,
      description: payload.description,
      type: payload.type || 'info',
      status: payload.status || 'unread',
      isPopup: payload.isPopup ?? true,
      duration: payload.duration ?? 3000,
      timestamp: Date.now()
    };

    if (existingIndex !== -1) {
      notifications.value[existingIndex] = newNotif;
    } else {
      notifications.value.unshift(newNotif);
    }

    if (newNotif.isPopup) {
      schedulePopupHide(id, newNotif.duration);
    }
  };

  const updateNotification = (id: string | number, payload: Partial<Omit<NotificationMessage, 'id' | 'timestamp'>>) => {
    const index = notifications.value.findIndex(n => n.id === id);
    if (index !== -1) {
      const existing = notifications.value[index];
      const updated = { ...existing, ...payload };
      notifications.value[index] = updated;

      if (updated.isPopup) {
        // Only reset the hide timer if isPopup was explicitly provided or it's not currently scheduled
        if (payload.isPopup === true || !popupTimeouts.has(id)) {
          schedulePopupHide(id, updated.duration);
        }
      } else {
        // If it's no longer a popup, clear any existing timeout
        clearPopupTimeout(id);
      }
    } else {
      // If it doesn't exist, create it
      addNotification({
        id,
        title: payload.title || 'Notification',
        description: payload.description,
        type: payload.type || 'info',
        isPopup: payload.isPopup ?? true,
        duration: payload.duration ?? 3000,
        status: payload.status || 'unread'
      });
    }
  };

  const schedulePopupHide = (id: string | number, duration: number) => {
    clearPopupTimeout(id);
    if (duration > 0) {
      const timeoutId = window.setTimeout(() => {
        const index = notifications.value.findIndex(n => n.id === id);
        if (index !== -1) {
          notifications.value[index].isPopup = false;
        }
        popupTimeouts.delete(id);
      }, duration);
      popupTimeouts.set(id, timeoutId);
    }
  };

  const clearPopupTimeout = (id: string | number) => {
    if (popupTimeouts.has(id)) {
      window.clearTimeout(popupTimeouts.get(id));
      popupTimeouts.delete(id);
    }
  };

  const removeNotification = (id: string | number) => {
    const index = notifications.value.findIndex((n) => n.id === id);
    if (index !== -1) {
      notifications.value.splice(index, 1);
    }
    clearPopupTimeout(id);
  };

  const clearExpired = () => {
    notifications.value = notifications.value.filter(n => n.status === 'unread' || n.isPopup);
  };

  const markAllAsRead = () => {
    notifications.value.forEach(n => {
      n.status = 'read';
    });
  };

  const dismissPopup = (id: string | number) => {
    const index = notifications.value.findIndex(n => n.id === id);
    if (index !== -1) {
      notifications.value[index].isPopup = false;
      // Also mark as read if user manually dismisses it
      notifications.value[index].status = 'read';
    }
    clearPopupTimeout(id);
  };

  // Helper methods for simple usages
  const success = (title: string, description?: string) => {
    addNotification({ title, description, type: 'success', isPopup: true, duration: 3000 });
  };

  const error = (title: string, description?: string) => {
    addNotification({ title, description, type: 'error', isPopup: true, duration: 4000 });
  };

  const info = (title: string, description?: string) => {
    addNotification({ title, description, type: 'info', isPopup: true, duration: 3000 });
  };

  return {
    notifications,
    isCenterOpen,
    unreadCount,
    addNotification,
    updateNotification,
    removeNotification,
    clearExpired,
    markAllAsRead,
    toggleCenter,
    dismissPopup,
    success,
    error,
    info,
  };
};

export const notificationStore = useNotificationStore();
