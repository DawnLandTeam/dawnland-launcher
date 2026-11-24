export type UpdateChannel = 'stable' | 'prerelease';

interface ParsedSemver {
  major: number;
  minor: number;
  patch: number;
  pre: string[];
}

const SEMVER_REGEX =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/;
const IDENTIFIER_REGEX = /^[0-9A-Za-z-]+$/;
const NUMERIC_IDENTIFIER_REGEX = /^(0|[1-9]\d*)$/;
const ALL_DIGITS_REGEX = /^\d+$/;

function parseLauncherSemver(raw: string): ParsedSemver | null {
  const trimmed = raw.trim();
  const normalized =
    trimmed.startsWith('v') || trimmed.startsWith('V') ? trimmed.slice(1) : trimmed;
  if (!normalized) {
    return null;
  }

  const match = SEMVER_REGEX.exec(normalized);
  if (!match) {
    return null;
  }

  const major = Number(match[1]);
  const minor = Number(match[2]);
  const patch = Number(match[3]);
  if (!Number.isSafeInteger(major) || !Number.isSafeInteger(minor) || !Number.isSafeInteger(patch)) {
    return null;
  }

  const preRaw = match[4];
  let pre: string[] = [];
  if (preRaw !== undefined) {
    pre = preRaw.split('.');
    for (const part of pre) {
      if (!part || !IDENTIFIER_REGEX.test(part)) {
        return null;
      }
      if (ALL_DIGITS_REGEX.test(part) && !NUMERIC_IDENTIFIER_REGEX.test(part)) {
        return null;
      }
    }
  }

  const buildRaw = match[5];
  if (buildRaw !== undefined) {
    const buildParts = buildRaw.split('.');
    for (const part of buildParts) {
      if (!part || !IDENTIFIER_REGEX.test(part)) {
        return null;
      }
    }
  }

  return { major, minor, patch, pre };
}

function compareNumericStrings(a: string, b: string): number {
  if (a.length !== b.length) {
    return a.length < b.length ? -1 : 1;
  }
  if (a === b) {
    return 0;
  }
  return a < b ? -1 : 1;
}

function compareParsedSemver(a: ParsedSemver, b: ParsedSemver): number {
  if (a.major !== b.major) {
    return a.major < b.major ? -1 : 1;
  }
  if (a.minor !== b.minor) {
    return a.minor < b.minor ? -1 : 1;
  }
  if (a.patch !== b.patch) {
    return a.patch < b.patch ? -1 : 1;
  }

  const aHasPre = a.pre.length > 0;
  const bHasPre = b.pre.length > 0;
  if (!aHasPre && !bHasPre) {
    return 0;
  }
  if (!aHasPre && bHasPre) {
    return 1;
  }
  if (aHasPre && !bHasPre) {
    return -1;
  }

  const len = Math.min(a.pre.length, b.pre.length);
  for (let i = 0; i < len; i++) {
    const partA = a.pre[i];
    const partB = b.pre[i];
    if (partA === partB) {
      continue;
    }

    const aIsNum = ALL_DIGITS_REGEX.test(partA);
    const bIsNum = ALL_DIGITS_REGEX.test(partB);
    if (aIsNum && bIsNum) {
      return compareNumericStrings(partA, partB);
    }
    if (aIsNum !== bIsNum) {
      return aIsNum ? -1 : 1;
    }
    return partA < partB ? -1 : 1;
  }

  if (a.pre.length === b.pre.length) {
    return 0;
  }
  return a.pre.length < b.pre.length ? -1 : 1;
}

export function normalizeUpdateChannel(channel: string | null): UpdateChannel {
  return channel === 'prerelease' ? 'prerelease' : 'stable';
}

export function getUpdateChannelQuery(): string {
  const channel = normalizeUpdateChannel(localStorage.getItem('updateChannel'));
  return channel === 'prerelease' ? '?channel=prerelease' : '';
}

export function compareLauncherVersions(a: string, b: string): number | null {
  const parsedA = parseLauncherSemver(a);
  const parsedB = parseLauncherSemver(b);
  if (!parsedA || !parsedB) {
    return null;
  }
  return compareParsedSemver(parsedA, parsedB);
}

export function isPrereleaseVersion(version: string): boolean {
  const parsed = parseLauncherSemver(version);
  return parsed !== null && parsed.pre.length > 0;
}

export function isLauncherUpdateAllowed(
  currentVersion: string,
  candidateVersion: string,
  channel: UpdateChannel,
): boolean {
  const current = parseLauncherSemver(currentVersion);
  const candidate = parseLauncherSemver(candidateVersion);
  if (!current || !candidate) {
    return false;
  }
  if (channel !== 'prerelease' && candidate.pre.length > 0) {
    return false;
  }
  return compareParsedSemver(candidate, current) === 1;
}
