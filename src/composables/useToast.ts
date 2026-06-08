import { notificationStore } from './useNotificationStore';

export const useToast = () => {
  const success = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'success',
      isPopup: true,
      status: 'read'
    });
  };

  const error = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'error',
      isPopup: true,
      status: 'read'
    });
  };

  const info = (title: string, description?: string) => {
    notificationStore.addNotification({
      title,
      description: description || '',
      type: 'info',
      isPopup: true,
      status: 'read'
    });
  };

  return {
    success,
    error,
    info,
  };
};

export const toast = useToast();
