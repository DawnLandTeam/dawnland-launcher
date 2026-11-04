export type InstanceStatus =
  | 'idle'
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

export function isInstanceBusy(
  instance:
    | {
        status?: InstanceStatus;
      }
    | null
    | undefined,
): boolean {
  return Boolean(instance?.status && instance.status !== 'idle');
}

export const INSTANCE_STATUS_LABELS: Record<InstanceStatus, string> = {
  idle: '',
  installing: 'instances.installing',
  cancellingInstall: 'instances.cancellingInstall',
  updating: 'instances.updating',
  cancellingUpdate: 'instances.cancellingUpdate',
};

export function getInstanceStatusLabel(status: InstanceStatus | string | undefined | null): string {
  if (!status) return '';
  return (INSTANCE_STATUS_LABELS as Record<string, string>)[status] || '';
}
