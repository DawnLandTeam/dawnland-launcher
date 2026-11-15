import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import {
  RESOURCE_NAMING_UPDATED_EVENT,
  type AssetCategory,
  type EnrichedModProject,
  type LauncherSettings,
  type ManagementDisplayStyle,
  type ResourceNamingSettings,
  type TranslationSearchCandidate,
  type UnifiedModProject,
} from '../types/resourceNaming';

export const DEFAULT_RESOURCE_NAMING_SETTINGS: ResourceNamingSettings = {
  filenameMode: 'auto',
  filenamePreset: 'bracketPrefix',
  customTemplate: '[{zh}] {stem}{ext}',
  renameTargets: ['Mod', 'ResourcePack', 'ShaderPack', 'Datapack'],
  managementDisplayStyle: 'translationFirst',
  enableOnlineTranslation: true,
  autoSyncTranslations: true,
};

const sharedNamingSettings = ref<ResourceNamingSettings>({
  ...DEFAULT_RESOURCE_NAMING_SETTINGS,
});
let initialized = false;

export function hasCjkCharacters(text: string): boolean {
  return /[\u3400-\u4dbf\u4e00-\u9fff]/.test(text);
}

export async function refreshResourceNamingSettings(): Promise<ResourceNamingSettings> {
  try {
    const settings = await invoke<LauncherSettings>('load_launcher_settings');
    if (settings.resourceNaming) {
      sharedNamingSettings.value = {
        ...DEFAULT_RESOURCE_NAMING_SETTINGS,
        ...settings.resourceNaming,
      };
    }
    initialized = true;
  } catch (err) {
    console.warn('Failed to load resource naming settings:', err);
  }
  return sharedNamingSettings.value;
}

export function notifyResourceNamingUpdated(next?: ResourceNamingSettings): void {
  if (next) {
    sharedNamingSettings.value = { ...DEFAULT_RESOURCE_NAMING_SETTINGS, ...next };
  }
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent(RESOURCE_NAMING_UPDATED_EVENT));
  }
}

export function useResourceNaming() {
  if (!initialized) {
    refreshResourceNamingSettings();
  }
  return {
    resourceNamingSettings: sharedNamingSettings,
    refreshResourceNamingSettings,
    notifyResourceNamingUpdated,
  };
}

export interface ResourceDisplayParts {
  primaryTitle: string;
  secondaryDetail: string;
  originalFilename: string;
  hasTranslation: boolean;
}

export function formatResourceDisplay(
  style: ManagementDisplayStyle,
  options: {
    zhName?: string | null;
    rawName?: string | null;
    filename: string;
    originalFilename?: string | null;
  },
  enableTranslation: boolean = sharedNamingSettings.value.enableOnlineTranslation,
): ResourceDisplayParts {
  const zh = (enableTranslation && options.zhName?.trim()) || '';
  const raw = options.rawName?.trim() || '';
  const fallbackName = raw || options.filename;
  const orig = options.originalFilename?.trim() || options.filename;

  if (style === 'filenameFirst') {
    const detail = zh ? `${zh} · ${orig !== options.filename ? orig : ''}`.replace(/ · $/, '') : (orig !== options.filename ? orig : '');
    return {
      primaryTitle: options.filename,
      secondaryDetail: detail,
      originalFilename: orig,
      hasTranslation: zh.length > 0,
    };
  }

  // TranslationFirst: title shows Chinese translation (or raw name), detail shows filename / original
  const primaryTitle = zh || fallbackName;
  const secondaryDetail =
    orig !== options.filename
      ? `${options.filename}`
      : primaryTitle === options.filename
        ? ''
        : options.filename;

  return {
    primaryTitle,
    secondaryDetail,
    originalFilename: orig,
    hasTranslation: zh.length > 0,
  };
}

export interface SearchWithTranslationParams {
  category: AssetCategory;
  searchCommand: string;
  args: {
    query: string;
    mcVersions: string[];
    loaders: string[];
    categories: string[];
    offset: number;
    limit: number;
  };
  resolveMissingOnline?: boolean;
}

export interface SearchWithTranslationResult {
  projects: EnrichedModProject[];
  translatedQueryUsed: string | null;
  candidates: TranslationSearchCandidate[];
}

export async function searchProjectsWithChineseSupport(
  params: SearchWithTranslationParams,
): Promise<SearchWithTranslationResult> {
  const rawQuery = params.args.query.trim();
  let effectiveQuery = rawQuery;
  let translatedQueryUsed: string | null = null;
  let candidates: TranslationSearchCandidate[] = [];

  if (rawQuery.length > 0 && hasCjkCharacters(rawQuery)) {
    try {
      candidates = await invoke<TranslationSearchCandidate[]>('search_translation_candidates', {
        category: params.category,
        query: rawQuery,
        limit: 10,
      });
      if (candidates.length > 0) {
        const top = candidates[0];
        const englishCandidate =
          top.rawName?.trim() ||
          top.mrSlug?.trim() ||
          top.cfSlug?.trim() ||
          '';
        if (englishCandidate) {
          effectiveQuery = englishCandidate;
          translatedQueryUsed = englishCandidate;
        }
      }
    } catch (err) {
      console.warn('Translation candidate search failed, using raw query:', err);
    }
  }

  const rawProjects = await invoke<UnifiedModProject[]>(params.searchCommand, {
    ...params.args,
    query: effectiveQuery,
  });

  if (rawProjects.length === 0) {
    return {
      projects: [],
      translatedQueryUsed,
      candidates,
    };
  }

  try {
    const enriched = await invoke<EnrichedModProject[]>('enrich_online_projects', {
      category: params.category,
      projects: rawProjects,
      resolveMissingOnline: params.resolveMissingOnline ?? params.args.offset === 0,
    });
    return {
      projects: enriched,
      translatedQueryUsed,
      candidates,
    };
  } catch (err) {
    console.warn('enrich_online_projects failed, falling back to raw results:', err);
    return {
      projects: rawProjects.map((p) => ({ ...p })),
      translatedQueryUsed,
      candidates,
    };
  }
}
