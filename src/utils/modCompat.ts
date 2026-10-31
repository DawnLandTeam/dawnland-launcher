import type { InspectedDropFile } from '../types';

export type ModIncompatReason = 'vanilla' | 'loader' | 'mcVersion';

export interface ModIncompatIssue {
  file: InspectedDropFile;
  reasons: ModIncompatReason[];
  modLoader?: string | null;
  requiredMcVersion?: string | null;
}

export interface CompatTargetInstance {
  loaderType: string;
  mcVersion: string;
}

/** Parse a Minecraft version like `1.20.1` / `26.1.2` into numeric parts. Snapshots return null. */
function parseVersion(v: string): number[] | null {
  const cleaned = v.trim().replace(/^v/i, '');
  if (!/^\d+(\.\d+)*$/.test(cleaned)) return null;
  return cleaned.split('.').map((p) => Number(p));
}

function compareVersions(a: number[], b: number[]): number {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}

/** Maven-style range used by Forge/NeoForge, e.g. `[1.20.1,1.21)` or `[1.20.1]`. */
function matchMavenRange(range: string, target: number[]): boolean | null {
  const segments = range.match(/[[(][^\])]*[\])]/g);
  if (!segments || segments.length === 0) return null;
  for (const seg of segments) {
    const lowerInc = seg.startsWith('[');
    const upperInc = seg.endsWith(']');
    const inner = seg.slice(1, -1);
    const [loRaw, hiRaw] = inner.includes(',') ? inner.split(',') : [inner, inner];
    const lo = loRaw.trim() ? parseVersion(loRaw) : null;
    const hi = hiRaw.trim() ? parseVersion(hiRaw) : null;
    if ((loRaw.trim() && !lo) || (hiRaw.trim() && !hi)) return null;
    let satisfies = true;
    if (lo) {
      const c = compareVersions(target, lo);
      satisfies = lowerInc ? c >= 0 : c > 0;
    }
    if (satisfies && hi) {
      const c = compareVersions(target, hi);
      satisfies = upperInc ? c <= 0 : c < 0;
    }
    if (satisfies) return true;
  }
  return false;
}

/** Single Fabric/Quilt style predicate, e.g. `>=1.20`, `~1.20.1`, `1.20.x`, `1.20.1`. */
function matchSemverPredicate(pred: string, target: number[]): boolean | null {
  const p = pred.trim();
  if (!p || p === '*') return true;
  const m = p.match(/^(>=|<=|>|<|=|~|\^)?\s*(.+)$/);
  if (!m) return null;
  const op = m[1] ?? '';
  const verStr = m[2].replace(/-.*$/, '');

  if (/[xX*]/.test(verStr)) {
    const parts = verStr.split('.');
    const fixed: number[] = [];
    for (const part of parts) {
      if (/^[xX*]$/.test(part)) break;
      if (!/^\d+$/.test(part)) return null;
      fixed.push(Number(part));
    }
    return fixed.every((n, i) => (target[i] ?? 0) === n);
  }

  const ver = parseVersion(verStr);
  if (!ver) return null;
  const c = compareVersions(target, ver);
  switch (op) {
    case '>=': return c >= 0;
    case '<=': return c <= 0;
    case '>': return c > 0;
    case '<': return c < 0;
    case '~': {
      if (c < 0) return false;
      const prefixLen = Math.max(ver.length - 1, 2);
      return ver.slice(0, prefixLen).every((n, i) => (target[i] ?? 0) === n);
    }
    case '^': {
      if (c < 0) return false;
      const major = ver[0] ?? 0;
      const minor = ver[1] ?? 0;
      const patch = ver[2] ?? 0;
      if (major > 0) {
        return (target[0] ?? 0) === major;
      }
      if (minor > 0) {
        return (target[0] ?? 0) === 0 && (target[1] ?? 0) === minor;
      }
      return (target[0] ?? 0) === 0 && (target[1] ?? 0) === 0 && (target[2] ?? 0) === patch;
    }
    default:
      // Exact version; treat `1.20` as matching `1.20` only.
      return c === 0;
  }
}

/**
 * Check whether the target MC version satisfies a mod's declared requirement.
 * Returns `null` when the requirement cannot be reliably parsed (no warning shown).
 */
export function isMcVersionSatisfied(requirement: string, mcVersion: string): boolean | null {
  const target = parseVersion(mcVersion);
  if (!target) return null;
  const req = requirement.trim();
  if (!req) return null;

  if (/^[[(]/.test(req)) {
    return matchMavenRange(req, target);
  }

  // `||` separated alternatives, each may be space separated AND predicates.
  let anyParsed = false;
  for (const alt of req.split('||')) {
    const preds = alt.trim().split(/\s+/).filter(Boolean);
    if (preds.length === 0) continue;
    let allOk = true;
    let parsed = true;
    for (const pred of preds) {
      const r = matchSemverPredicate(pred, target);
      if (r === null) { parsed = false; break; }
      if (!r) allOk = false;
    }
    if (!parsed) continue;
    anyParsed = true;
    if (allOk) return true;
  }
  return anyParsed ? false : null;
}

function isLoaderCompatible(modLoader: string, instanceLoader: string): boolean {
  const m = modLoader.toLowerCase();
  const i = instanceLoader.toLowerCase();
  if (m === i) return true;
  // Quilt can load Fabric mods
  if (i === 'quilt' && m === 'fabric') return true;
  return false;
}

/** Collect incompatibility issues for dropped Mod files against the target instance. */
export function findIncompatibleMods(
  files: InspectedDropFile[],
  instance: CompatTargetInstance,
): ModIncompatIssue[] {
  const issues: ModIncompatIssue[] = [];
  const instLoader = (instance.loaderType || '').toLowerCase();

  for (const file of files) {
    if (file.kind !== 'Mod') continue;
    const reasons: ModIncompatReason[] = [];

    if (instLoader === 'vanilla' || instLoader === '') {
      reasons.push('vanilla');
    } else if (file.detectedLoader && !isLoaderCompatible(file.detectedLoader, instLoader)) {
      reasons.push('loader');
    }

    if (file.detectedMcVersion && instance.mcVersion) {
      const ok = isMcVersionSatisfied(file.detectedMcVersion, instance.mcVersion);
      if (ok === false) reasons.push('mcVersion');
    }

    if (reasons.length > 0) {
      issues.push({
        file,
        reasons,
        modLoader: file.detectedLoader,
        requiredMcVersion: file.detectedMcVersion,
      });
    }
  }
  return issues;
}
