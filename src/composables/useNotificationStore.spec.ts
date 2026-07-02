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

  it('manually dismisses transient popup completely', () => {
    const store = useNotificationStore();
    
    store.addNotification({ id: 'transient-1', title: 'Test', isPopup: true, duration: 5000, transient: true });
    expect(store.notifications.value).toHaveLength(1);
    
    store.dismissPopup('transient-1');
    expect(store.notifications.value).toHaveLength(0);
  });

  it('removes notification completely', () => {
    const store = useNotificationStore();
    store.addNotification({ id: 'rem-1', title: 'Test' });
    expect(store.notifications.value).toHaveLength(1);
    
    store.removeNotification('rem-1');
    expect(store.notifications.value).toHaveLength(0);
  });

  it('clears expired (read and not popup) notifications', () => {
    const store = useNotificationStore();
    
    store.addNotification({ id: 'c-1', title: 'Keep unread', status: 'unread', isPopup: false });
    store.addNotification({ id: 'c-2', title: 'Keep popup', status: 'read', isPopup: true });
    store.addNotification({ id: 'c-3', title: 'Remove me', status: 'read', isPopup: false });
    
    expect(store.notifications.value).toHaveLength(3);
    
    store.clearExpired();
    
    expect(store.notifications.value).toHaveLength(2);
    expect(store.notifications.value.find(n => n.id === 'c-3')).toBeUndefined();
  });
});
