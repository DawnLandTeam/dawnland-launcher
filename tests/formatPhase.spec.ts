import { describe, it, expect } from 'vitest';

function formatPhase(phase: string): string {
  const labels: Record<string, string> = {
    resolving_version: "Fetching version metadata...",
    resolving_libraries: "Filtering libraries for your system...",
    resolving_assets: "Preparing game assets...",
    downloading: "Downloading files...",
    complete: "Installation complete!",
    error: "Installation failed",
  };
  return labels[phase] || phase;
}

describe('formatPhase', () => {
  it('formats known phases correctly', () => {
    expect(formatPhase('resolving_version')).toBe('Fetching version metadata...');
    expect(formatPhase('downloading')).toBe('Downloading files...');
  });

  it('returns raw phase string if unknown', () => {
    expect(formatPhase('unknown_phase')).toBe('unknown_phase');
  });
});
