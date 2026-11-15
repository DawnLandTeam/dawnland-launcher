import { describe, expect, it, vi, beforeEach } from 'vitest';
import {
  formatResourceDisplay,
  hasCjkCharacters,
  searchProjectsWithChineseSupport,
} from './useResourceNaming';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

describe('useResourceNaming helpers', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('detects CJK characters accurately', () => {
    expect(hasCjkCharacters('机械动力')).toBe(true);
    expect(hasCjkCharacters('JEI 物品管理器')).toBe(true);
    expect(hasCjkCharacters('create mod')).toBe(false);
  });

  it('switches between translationFirst and filenameFirst display modes', () => {
    const sample = {
      zhName: '机械动力',
      rawName: 'Create',
      filename: '[机械动力] create-1.21.1-6.0.4.jar',
      originalFilename: 'create-1.21.1-6.0.4.jar',
    };

    const tf = formatResourceDisplay('translationFirst', sample);
    expect(tf.primaryTitle).toBe('机械动力');
    expect(tf.secondaryDetail).toBe('[机械动力] create-1.21.1-6.0.4.jar');

    const ff = formatResourceDisplay('filenameFirst', sample);
    expect(ff.primaryTitle).toBe('[机械动力] create-1.21.1-6.0.4.jar');
    expect(ff.secondaryDetail).toBe('机械动力 · create-1.21.1-6.0.4.jar');
  });

  it('performs Chinese-to-English search augmentation and enriches results', async () => {
    invokeMock
      .mockResolvedValueOnce([
        {
          category: 'mod',
          rawName: 'Create',
          mrSlug: 'create',
          cfSlug: 'create',
          zhName: '机械动力 (Create)',
          zhShortName: '机械动力',
        },
      ])
      .mockResolvedValueOnce([
        {
          source: 'modrinth',
          project_id: 'LNytGkhc',
          slug: 'create',
          title: 'Create',
          description: 'Aesthetic technology',
          downloads: 1000,
          author: 'simibubi',
          mc_versions: ['1.21.1'],
          loaders: ['neoforge'],
        },
      ])
      .mockResolvedValueOnce([
        {
          source: 'modrinth',
          project_id: 'LNytGkhc',
          slug: 'create',
          title: 'Create',
          description: 'Aesthetic technology',
          downloads: 1000,
          author: 'simibubi',
          mc_versions: ['1.21.1'],
          loaders: ['neoforge'],
          zhTitle: '机械动力 (Create)',
          zhSummary: '用齿轮和动力建造自动化工厂',
        },
      ]);

    const res = await searchProjectsWithChineseSupport({
      category: 'Mod',
      searchCommand: 'search_modrinth',
      args: {
        query: '机械动力',
        mcVersions: ['1.21.1'],
        loaders: ['neoforge'],
        categories: [],
        offset: 0,
        limit: 20,
      },
    });

    expect(res.translatedQueryUsed).toBe('Create');
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'search_translation_candidates', {
      category: 'Mod',
      query: '机械动力',
      limit: 10,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'search_modrinth', {
      query: 'Create',
      mcVersions: ['1.21.1'],
      loaders: ['neoforge'],
      categories: [],
      offset: 0,
      limit: 20,
    });
    expect(res.projects[0]?.zhTitle).toBe('机械动力 (Create)');
  });
});
