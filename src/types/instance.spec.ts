import { describe, it, expect } from 'vitest';
import { isInstanceBusy, INSTANCE_STATUS_LABELS, type InstanceStatus } from './instance';

describe('instance state machine helpers', () => {
  it('identifies busy states accurately', () => {
    expect(isInstanceBusy(null)).toBe(false);
    expect(isInstanceBusy(undefined)).toBe(false);
    expect(isInstanceBusy({})).toBe(false);
    expect(isInstanceBusy({ status: 'idle' })).toBe(false);

    expect(isInstanceBusy({ status: 'installing' })).toBe(true);
    expect(isInstanceBusy({ status: 'cancellingInstall' })).toBe(true);
    expect(isInstanceBusy({ status: 'updating' })).toBe(true);
    expect(isInstanceBusy({ status: 'cancellingUpdate' })).toBe(true);
  });

  it('provides internationalized status keys for all non-idle statuses', () => {
    const statuses: InstanceStatus[] = [
      'idle',
      'installing',
      'cancellingInstall',
      'updating',
      'cancellingUpdate',
    ];

    statuses.forEach((status) => {
      expect(typeof INSTANCE_STATUS_LABELS[status]).toBe('string');
    });

    expect(INSTANCE_STATUS_LABELS.idle).toBe('');
    expect(INSTANCE_STATUS_LABELS.installing).toBe('instances.installing');
    expect(INSTANCE_STATUS_LABELS.cancellingInstall).toBe('instances.cancellingInstall');
    expect(INSTANCE_STATUS_LABELS.updating).toBe('instances.updating');
    expect(INSTANCE_STATUS_LABELS.cancellingUpdate).toBe('instances.cancellingUpdate');
  });
});
