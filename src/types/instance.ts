export type InstanceStatus =
  | 'ready'
  | 'running'
  | 'installing'
  | 'cancellingInstall'
  | 'updating'
  | 'cancellingUpdate';

export interface InstanceItem {
  id: string;
  name: string;
  path?: string;
  mcVersion: string;
  loaderType: string;
  modpackVersion?: string;
  modpackType?: string;
  modpackProjectId?: string;
  serverId?: string;
  packVersionId?: string;
  packFileName?: string;
  status: InstanceStatus;
  iconPath?: string | null;
}

export const BUSY_INSTANCE_STATUSES: readonly InstanceStatus[] = [
  'installing',
  'cancellingInstall',
  'updating',
  'cancellingUpdate',
];

export function isInstanceBusy(
  instance:
    | {
        status?: InstanceStatus;
      }
    | null
    | undefined,
): boolean {
  if (!instance?.status) return false;
  return BUSY_INSTANCE_STATUSES.includes(instance.status);
}

export function isInstanceRunning(
  instance:
    | {
        status?: InstanceStatus;
      }
    | null
    | undefined,
): boolean {
  return instance?.status === 'running';
}

export function isInstanceReady(
  instance:
    | {
        status?: InstanceStatus;
      }
    | null
    | undefined,
): boolean {
  return instance?.status === 'ready';
}

export const INSTANCE_STATUS_LABELS: Record<InstanceStatus, string> = {
  ready: '',
  running: 'instances.running',
  installing: 'instances.installing',
  cancellingInstall: 'instances.cancellingInstall',
  updating: 'instances.updating',
  cancellingUpdate: 'instances.cancellingUpdate',
};

export function getInstanceStatusLabel(status: InstanceStatus | string | undefined | null): string {
  if (!status) return '';
  return (INSTANCE_STATUS_LABELS as Record<string, string>)[status] || '';
}
