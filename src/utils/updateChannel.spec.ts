import { describe, it, expect } from 'vitest';
import {
  normalizeUpdateChannel,
  compareLauncherVersions,
  isPrereleaseVersion,
  isLauncherUpdateAllowed,
} from './updateChannel';

describe('updateChannel SemVer gating', () => {
  it('normalizes update channel strictly to stable or prerelease', () => {
    expect(normalizeUpdateChannel(null)).toBe('stable');
    expect(normalizeUpdateChannel('stable')).toBe('stable');
    expect(normalizeUpdateChannel('beta')).toBe('stable');
    expect(normalizeUpdateChannel('prerelease')).toBe('prerelease');
  });

  it('compares SemVer 2.0.0 versions accurately including prerelease precedence', () => {
    expect(compareLauncherVersions('0.1.6-rc.4', '0.1.5')).toBe(1);
    expect(compareLauncherVersions('0.1.5', '0.1.6-rc.4')).toBe(-1);
    expect(compareLauncherVersions('0.1.6-rc.3', '0.1.6-rc.4')).toBe(-1);
    expect(compareLauncherVersions('0.1.6-rc.4', '0.1.6')).toBe(-1);
    expect(compareLauncherVersions('0.1.6', '0.1.6-rc.4')).toBe(1);
    expect(compareLauncherVersions('0.1.6-rc.9', '0.1.6-rc.10')).toBe(-1);
    expect(compareLauncherVersions('0.1.6-rc', '0.1.6-rc.1')).toBe(-1);
    expect(compareLauncherVersions('0.1.6-1', '0.1.6-alpha')).toBe(-1);
    expect(compareLauncherVersions(' V0.1.6-rc.4 ', '0.1.6-rc.4')).toBe(0);
    expect(compareLauncherVersions('0.1.6+a', '0.1.6+b')).toBe(0);

    // Rejects invalid SemVer formats
    expect(compareLauncherVersions('0.1.6-rc.01', '0.1.6')).toBeNull();
    expect(compareLauncherVersions('vv0.1.6', '0.1.6')).toBeNull();
    expect(compareLauncherVersions('0.1.6-rc.', '0.1.6')).toBeNull();
    expect(compareLauncherVersions('0.1', '0.1.6')).toBeNull();
    expect(compareLauncherVersions('', '0.1.6')).toBeNull();
  });

  it('detects prerelease versions properly', () => {
    expect(isPrereleaseVersion('0.1.6-rc.4')).toBe(true);
    expect(isPrereleaseVersion('v0.1.6-rc')).toBe(true);
    expect(isPrereleaseVersion('0.1.6')).toBe(false);
    expect(isPrereleaseVersion('0.1.6+build.1')).toBe(false);
    expect(isPrereleaseVersion('invalid')).toBe(false);
  });

  it('enforces upgrade-only and channel-aware admission matrix', () => {
    // Core regression case: 0.1.6-rc.4 must NEVER downgrade to 0.1.5
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', '0.1.5', 'stable')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', '0.1.5', 'prerelease')).toBe(false);

    // Allowed progression: 0.1.6-rc.3 -> 0.1.6-rc.4 -> 0.1.6
    expect(isLauncherUpdateAllowed('0.1.6-rc.3', '0.1.6-rc.4', 'prerelease')).toBe(true);
    expect(isLauncherUpdateAllowed('0.1.6-rc.3', '0.1.6-rc.4', 'stable')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', '0.1.6', 'stable')).toBe(true);
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', '0.1.6', 'prerelease')).toBe(true);

    // Disallow downgrades and same-version updates
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', '0.1.6-rc.3', 'prerelease')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6', '0.1.6-rc.4', 'stable')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6', '0.1.6-rc.4', 'prerelease')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6-rc.4', 'v0.1.6-rc.4', 'prerelease')).toBe(false);
    expect(isLauncherUpdateAllowed('0.1.6+a', '0.1.6+b', 'stable')).toBe(false);

    // SemVer 2.0.0 prerelease edge cases
    expect(isLauncherUpdateAllowed('0.1.6-rc.9', '0.1.6-rc.10', 'prerelease')).toBe(true);
    expect(isLauncherUpdateAllowed('0.1.6-rc', '0.1.6-rc.1', 'prerelease')).toBe(true);

    // Fail-closed on unparseable versions
    expect(isLauncherUpdateAllowed('0.0.0', 'invalid', 'stable')).toBe(false);
    expect(isLauncherUpdateAllowed('', '0.1.6', 'stable')).toBe(false);
  });
});
