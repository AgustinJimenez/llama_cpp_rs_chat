import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import en from './locales/en.json';

const STORAGE_KEY = 'language';

i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: localStorage.getItem(STORAGE_KEY) || 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
});

export default i18n;
