import { describe, it, expect } from 'vitest';
import { isSafeInstanceName } from './instanceName';

describe('isSafeInstanceName', () => {
  it('accepts valid instance names', () => {
    expect(isSafeInstanceName('1.20.1')).toBe(true);
    expect(isSafeInstanceName('My Survival Pack')).toBe(true);
    expect(isSafeInstanceName('Dawnland-v1.0_beta')).toBe(true);
    expect(isSafeInstanceName('中文实例名')).toBe(true);
  });

  it('rejects empty, dot-only, and trailing-dot/space names', () => {
    expect(isSafeInstanceName('')).toBe(false);
    expect(isSafeInstanceName('.')).toBe(false);
    expect(isSafeInstanceName('..')).toBe(false);
    expect(isSafeInstanceName('...')).toBe(false);
    expect(isSafeInstanceName('trailing.')).toBe(false);
    expect(isSafeInstanceName('trailing ')).toBe(false);
  });

  it('rejects path separators and invalid characters', () => {
    expect(isSafeInstanceName('foo/bar')).toBe(false);
    expect(isSafeInstanceName('foo\\bar')).toBe(false);
    expect(isSafeInstanceName('foo:bar')).toBe(false);
    expect(isSafeInstanceName('foo*bar')).toBe(false);
    expect(isSafeInstanceName('foo?bar')).toBe(false);
    expect(isSafeInstanceName('foo"bar')).toBe(false);
    expect(isSafeInstanceName('foo<bar')).toBe(false);
    expect(isSafeInstanceName('foo>bar')).toBe(false);
    expect(isSafeInstanceName('foo|bar')).toBe(false);
    expect(isSafeInstanceName('foo\0bar')).toBe(false);
  });

  it('rejects Windows reserved device names', () => {
    expect(isSafeInstanceName('CON')).toBe(false);
    expect(isSafeInstanceName('con')).toBe(false);
    expect(isSafeInstanceName('nul.txt')).toBe(false);
    expect(isSafeInstanceName('COM1')).toBe(false);
    expect(isSafeInstanceName('LPT9')).toBe(false);
    expect(isSafeInstanceName('CON.pack')).toBe(false);
  });
});
