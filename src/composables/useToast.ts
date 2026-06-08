import { toast as sonnerToast } from 'vue-sonner';

export const useToast = () => {
  const success = (title: string, description?: string) => {
    sonnerToast.success(title, { description });
  };

  const error = (title: string, description?: string) => {
    sonnerToast.error(title, { description });
  };

  const info = (title: string, description?: string) => {
    sonnerToast.info(title, { description });
  };

  return {
    success,
    error,
    info,
  };
};

export const toast = useToast();
