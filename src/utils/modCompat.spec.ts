import { describe, it, expect } from 'vitest';
import { isMcVersionSatisfied, findIncompatibleMods } from './modCompat';
import type { InspectedDropFile } from '../types';

function mod(partial: Partial<InspectedDropFile>): InspectedDropFile {
  return {
    path: `C:\\mods\\${partial.filename ?? 'a.jar'}`,
    filename: 'a.jar',
    kind: 'Mod',
    size: 1,
    isDir: false,
    ...partial,
  };
}

describe('isMcVersionSatisfied', () => {
  it('handles Forge maven ranges', () => {
    expect(isMcVersionSatisfied('[1.20.1,1.21)', '1.20.1')).toBe(true);
    expect(isMcVersionSatisfied('[1.20.1,1.21)', '1.21')).toBe(false);
    expect(isMcVersionSatisfied('[1.21.1]', '1.21.1')).toBe(true);
    expect(isMcVersionSatisfied('[1.21,)', '26.1.2')).toBe(true);
  });

  it('handles Fabric semver predicates', () => {
    expect(isMcVersionSatisfied('>=1.20', '1.20.4')).toBe(true);
    expect(isMcVersionSatisfied('~1.20.1', '1.20.4')).toBe(true);
    expect(isMcVersionSatisfied('~1.20.1', '1.21')).toBe(false);
    expect(isMcVersionSatisfied('1.20.x', '1.20.6')).toBe(true);
    expect(isMcVersionSatisfied('1.21.1', '1.20.1')).toBe(false);
    expect(isMcVersionSatisfied('>=1.20 <1.21', '1.21.1')).toBe(false);
    expect(isMcVersionSatisfied('1.20.1 || 1.20.4', '1.20.4')).toBe(true);
    expect(isMcVersionSatisfied('^1.20.1', '1.21.0')).toBe(true);
    expect(isMcVersionSatisfied('^1.20.1', '2.0.0')).toBe(false);
    expect(isMcVersionSatisfied('^1.20.1', '1.20.0')).toBe(false);
  });

  it('returns null for unparseable input', () => {
    expect(isMcVersionSatisfied('>=1.20', '24w10a')).toBeNull();
    expect(isMcVersionSatisfied('weird', '1.20.1')).toBeNull();
  });
});

describe('findIncompatibleMods', () => {
  it('flags vanilla, loader and mc version mismatches', () => {
    const files = [
      mod({ filename: 'fab.jar', detectedLoader: 'Fabric', detectedMcVersion: '~1.20.1' }),
      mod({ filename: 'neo.jar', detectedLoader: 'NeoForge', detectedMcVersion: '[1.21.1,1.22)' }),
      mod({ filename: 'neo-old.jar', detectedLoader: 'NeoForge', detectedMcVersion: '[1.20.4,1.20.5)' }),
    ];
    const issues = findIncompatibleMods(files, { loaderType: 'NeoForge', mcVersion: '1.21.1' });
    expect(issues.map((i) => i.file.filename)).toEqual(['fab.jar', 'neo-old.jar']);
    expect(issues[0].reasons).toEqual(['loader', 'mcVersion']);
    expect(issues[1].reasons).toEqual(['mcVersion']);

    const vanilla = findIncompatibleMods([files[1]], { loaderType: 'Vanilla', mcVersion: '1.21.1' });
    expect(vanilla[0].reasons).toEqual(['vanilla']);
  });

  it('allows Fabric mods on Quilt', () => {
    const issues = findIncompatibleMods(
      [mod({ detectedLoader: 'Fabric', detectedMcVersion: '>=1.20' })],
      { loaderType: 'Quilt', mcVersion: '1.20.1' },
    );
    expect(issues).toHaveLength(0);
  });
});
