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
  
  const lang = navigator.language;
  if (lang.startsWith('zh')) {
    return 'zh-CN';
  }
  return 'en';
}

const i18n = createI18n({
  legacy: false, // Use Composition API
  locale: getBrowserLanguage(),
  fallbackLocale: 'en',
  messages
});

export default i18n;
