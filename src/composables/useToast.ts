import { notificationStore } from './useNotificationStore';

export const useToast = () => {
  const success = (title: string, description?: string, options?: { transient?: boolean }) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'success',
      isPopup: true,
      status: 'read',
      duration: 3000,
      transient: options?.transient
    });
  };

  const error = (title: string, description?: string, options?: { transient?: boolean }) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'error',
      isPopup: true,
      status: 'read',
      duration: 3000,
      transient: options?.transient
    });
  };

  const info = (title: string, description?: string, options?: { transient?: boolean }) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'info',
      isPopup: true,
      status: 'read',
      duration: 3000,
      transient: options?.transient
    });
  };

  return {
    success,
    error,
    info,
  };
};

export const toast = useToast();
