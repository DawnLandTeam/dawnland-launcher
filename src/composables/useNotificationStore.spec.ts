import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useNotificationStore } from './useNotificationStore';

describe('useNotificationStore composable', () => {
  beforeEach(() => {
    const store = useNotificationStore();
    // Reset global state
    store.notifications.value = [];
    store.isCenterOpen.value = false;
    vi.useFakeTimers();
  });

  it('adds a notification and schedules a popup timeout', () => {
    const store = useNotificationStore();
    
    store.addNotification({
      title: 'Test Notif',
      type: 'info',
      isPopup: true,
      duration: 3000
    });
    
    expect(store.notifications.value).toHaveLength(1);
    expect(store.notifications.value[0].title).toBe('Test Notif');
    expect(store.notifications.value[0].isPopup).toBe(true);
    
    // Fast-forward timers
    vi.advanceTimersByTime(3000);
    
    // After timeout, popup should be false
    expect(store.notifications.value[0].isPopup).toBe(false);
  });

  it('updates an existing notification', () => {
    const store = useNotificationStore();
    
    store.addNotification({ id: 'task-1', title: 'Running', duration: 3000, isPopup: true });
    expect(store.notifications.value[0].title).toBe('Running');
    
    store.updateNotification('task-1', { title: 'Completed', type: 'success' });
    
    expect(store.notifications.value).toHaveLength(1);
    expect(store.notifications.value[0].title).toBe('Completed');
    expect(store.notifications.value[0].type).toBe('success');
  });

  it('removes transient notifications on timeout', () => {
    const store = useNotificationStore();
    
    store.addNotification({
      title: 'Transient Notif',
      isPopup: true,
      duration: 1000,
      transient: true
    });
    
    expect(store.notifications.value).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    
    // Should be removed completely, not just hidden
    expect(store.notifications.value).toHaveLength(0);
  });

  it('calculates unread count correctly and clears them when opening center', () => {
    const store = useNotificationStore();
    
    store.info('Test 1');
    store.error('Test 2');
    
    expect(store.unreadCount.value).toBe(2);
    
    // Toggle center open
    store.toggleCenter();
    
    expect(store.isCenterOpen.value).toBe(true);
    expect(store.unreadCount.value).toBe(0);
    expect(store.notifications.value[0].status).toBe('read');
  });

  it('manually dismisses popup', () => {
    const store = useNotificationStore();
    
    store.addNotification({ id: 'p1', title: 'Test', isPopup: true, duration: 5000 });
    expect(store.notifications.value[0].isPopup).toBe(true);
    
    store.dismissPopup('p1');
    expect(store.notifications.value[0].isPopup).toBe(false);
    expect(store.notifications.value[0].status).toBe('read');
  });
});
