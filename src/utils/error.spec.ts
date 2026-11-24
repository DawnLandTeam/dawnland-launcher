import { describe, it, expect, vi } from 'vitest';
import { getErrorMessage } from './error';

vi.mock('../i18n', () => ({
  default: {
    global: {
      t: vi.fn((key: string, args?: any) => {
        if (args) return `${key} ${JSON.stringify(args)}`;
        return key;
      })
    }
  }
}));

describe('getErrorMessage utility', () => {
  it('handles null/undefined', () => {
    expect(getErrorMessage(null)).toBe('Unknown error');
    expect(getErrorMessage(undefined)).toBe('Unknown error');
  });

  it('handles MD5_MISMATCH code', () => {
    expect(getErrorMessage({ code: 'MD5_MISMATCH' })).toBe('errors.md5Mismatch');
  });

  it('handles UPDATE_VERSION_NOT_NEWER code', () => {
    expect(getErrorMessage({ code: 'UPDATE_VERSION_NOT_NEWER' })).toBe('errors.updateVersionNotNewer');
  });

  it('handles INVALID_IMAGE_FORMAT and IMAGE_TOO_LARGE codes', () => {
    expect(getErrorMessage({ code: 'INVALID_IMAGE_FORMAT' })).toBe('errors.invalidImageFormat');
    expect(getErrorMessage({ code: 'IMAGE_TOO_LARGE' })).toBe('errors.imageTooLarge');
    expect(getErrorMessage({ code: 'IMAGE_DIMENSIONS_TOO_LARGE' })).toBe('errors.imageDimensionsTooLarge');
  });

  it('handles object with message string', () => {
    expect(getErrorMessage({ message: 'Custom message' })).toBe('Custom message');
  });

  it('handles object with data string', () => {
    expect(getErrorMessage({ data: 'Data message' })).toBe('Data message');
  });

  it('handles simple string', () => {
    expect(getErrorMessage('String error')).toBe('String error');
  });

  it('handles other types (fallback to String)', () => {
    expect(getErrorMessage(1234)).toBe('1234');
    expect(getErrorMessage({ foo: 'bar' })).toBe('Unknown error');
  });

  it('handles specific Database error CONFLICTING_TASK', () => {
    expect(getErrorMessage('Database error: CONFLICTING_TASK: InstallVanilla')).toBe('errors.conflictingTask {"taskName":"InstallVanilla"}');
  });

  it('handles specific settings access denied error', () => {
    expect(getErrorMessage('Failed to write launcher settings due to os error 5')).toBe('errors.settingsAccessDenied');
  });

  it('handles specific instance exists error', () => {
    expect(getErrorMessage("Instance with name 'MyInstance' already exists")).toBe('errors.instanceExists {"instanceName":"MyInstance"}');
  });
});
