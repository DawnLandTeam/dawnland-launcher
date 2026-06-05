import { createI18n } from 'vue-i18n';
import en from './locales/en.json';
import zhCN from './locales/zh-CN.json';

const messages = {
  'en': en,
  'zh-CN': zhCN
};

function getBrowserLanguage(): string {
  const savedLang = localStorage.getItem('language');
  const isUserSelected = localStorage.getItem('userSelectedLanguage');
  
  // Only strictly respect the saved language if the user manually selected it
  if (savedLang && isUserSelected === 'true') {
    return savedLang;
  }
  
  // Use Intl.DateTimeFormat as a fast initial guess, but don't cache it!
  const lang = Intl.DateTimeFormat().resolvedOptions().locale || navigator.language;
  return lang.startsWith('zh') ? 'zh-CN' : 'en';
}

const i18n = createI18n({
  legacy: false, // Use Composition API
  locale: getBrowserLanguage(),
  fallbackLocale: 'en',
  messages
});

export default i18n;
