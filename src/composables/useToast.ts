import { notificationStore } from './useNotificationStore';

export const useToast = () => {
  const success = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'success',
      isPopup: true,
      status: 'read',
      duration: 3000
    });
  };

  const error = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'error',
      isPopup: true,
      status: 'read',
      duration: 3000
    });
  };

  const info = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'info',
      isPopup: true,
      status: 'read',
      duration: 3000
    });
  };

  return {
    success,
    error,
    info,
  };
};

export const toast = useToast();
