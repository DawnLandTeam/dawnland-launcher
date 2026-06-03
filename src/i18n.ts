import { createI18n } from 'vue-i18n';
import en from './locales/en.json';
import zhCN from './locales/zh-CN.json';

const messages = {
  'en': en,
  'zh-CN': zhCN
};

function getBrowserLanguage(): string {
  const savedLang = localStorage.getItem('language');
  if (savedLang && Object.keys(messages).includes(savedLang)) {
    return savedLang;
  }
  
  // Use Intl.DateTimeFormat to get the system locale which bypasses WebView2's en-US default bug
  const lang = Intl.DateTimeFormat().resolvedOptions().locale || navigator.language;
  const detectedLang = lang.startsWith('zh') ? 'zh-CN' : 'en';
  
  // Save it immediately so it doesn't try to detect again
  localStorage.setItem('language', detectedLang);
  
  return detectedLang;
}

const i18n = createI18n({
  legacy: false, // Use Composition API
  locale: getBrowserLanguage(),
  fallbackLocale: 'en',
  messages
});

export default i18n;
