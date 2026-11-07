import { describe, it, expect } from 'vitest';
import { formatBytes, formatSpeed } from './format';

describe('format utilities', () => {
  describe('formatBytes', () => {
    it('returns "0 B" for 0 or negative or invalid values', () => {
      expect(formatBytes(0)).toBe('0 B');
      expect(formatBytes(-10)).toBe('0 B');
      expect(formatBytes(undefined)).toBe('0 B');
      expect(formatBytes(null)).toBe('0 B');
      expect(formatBytes(NaN)).toBe('0 B');
    });

    it('formats bytes correctly', () => {
      expect(formatBytes(500)).toBe('500 B');
      expect(formatBytes(1024)).toBe('1 KB');
      expect(formatBytes(1536)).toBe('1.5 KB');
      expect(formatBytes(1024 * 1024)).toBe('1 MB');
      expect(formatBytes(1024 * 1024 * 1024)).toBe('1 GB');
    });
  });

  describe('formatSpeed', () => {
    it('returns empty string for 0, negative or invalid speeds', () => {
      expect(formatSpeed(0)).toBe('');
      expect(formatSpeed(-100)).toBe('');
      expect(formatSpeed(undefined)).toBe('');
      expect(formatSpeed(null)).toBe('');
      expect(formatSpeed(NaN)).toBe('');
    });

    it('formats speed with correct units', () => {
      expect(formatSpeed(500)).toBe('500 B/s');
      expect(formatSpeed(1024)).toBe('1 KB/s');
      expect(formatSpeed(1536)).toBe('1.5 KB/s');
      expect(formatSpeed(5 * 1024 * 1024)).toBe('5 MB/s');
      expect(formatSpeed(12.5 * 1024 * 1024)).toBe('12.5 MB/s');
    });
  });
});
