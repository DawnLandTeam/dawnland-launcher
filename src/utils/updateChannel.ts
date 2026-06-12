export function normalizeUpdateChannel(channel: string | null): 'stable' | 'prerelease' {
  return channel === 'prerelease' ? 'prerelease' : 'stable';
}

export function getUpdateChannelQuery(): string {
  const channel = normalizeUpdateChannel(localStorage.getItem('updateChannel'));
  return channel === 'prerelease' ? '?channel=prerelease' : '';
}
