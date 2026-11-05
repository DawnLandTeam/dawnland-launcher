import { describe, it, expect } from 'vitest';
import {
  isInstanceBusy,
  isInstanceRunning,
  isInstanceReady,
  INSTANCE_STATUS_LABELS,
  type InstanceStatus,
} from './instance';

describe('instance state machine helpers', () => {
  it('identifies busy and runtime states accurately', () => {
    expect(isInstanceBusy(null)).toBe(false);
    expect(isInstanceBusy(undefined)).toBe(false);
    expect(isInstanceBusy({})).toBe(false);
    expect(isInstanceBusy({ status: 'ready' })).toBe(false);
    expect(isInstanceBusy({ status: 'running' })).toBe(false);

    expect(isInstanceRunning({ status: 'running' })).toBe(true);
    expect(isInstanceRunning({ status: 'ready' })).toBe(false);

    expect(isInstanceReady({ status: 'ready' })).toBe(true);
    expect(isInstanceReady({ status: 'running' })).toBe(false);

    expect(isInstanceBusy({ status: 'installing' })).toBe(true);
    expect(isInstanceBusy({ status: 'cancellingInstall' })).toBe(true);
    expect(isInstanceBusy({ status: 'updating' })).toBe(true);
    expect(isInstanceBusy({ status: 'cancellingUpdate' })).toBe(true);
  });

  it('provides internationalized status keys for all statuses', () => {
    const statuses: InstanceStatus[] = [
      'ready',
      'running',
      'installing',
      'cancellingInstall',
      'updating',
      'cancellingUpdate',
    ];

    statuses.forEach((status) => {
      expect(typeof INSTANCE_STATUS_LABELS[status]).toBe('string');
    });

    expect(INSTANCE_STATUS_LABELS.ready).toBe('');
    expect(INSTANCE_STATUS_LABELS.running).toBe('instances.running');
    expect(INSTANCE_STATUS_LABELS.installing).toBe('instances.installing');
    expect(INSTANCE_STATUS_LABELS.cancellingInstall).toBe('instances.cancellingInstall');
    expect(INSTANCE_STATUS_LABELS.updating).toBe('instances.updating');
    expect(INSTANCE_STATUS_LABELS.cancellingUpdate).toBe('instances.cancellingUpdate');
  });
});
