import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import ru from "./locales/ru.json";

export function systemLanguage(locale: string): "ru" | "en" {
  return locale.toLowerCase().startsWith("ru") ? "ru" : "en";
}

export function initI18n() {
  const lng = systemLanguage(navigator.language);
  document.documentElement.lang = lng;
  void i18n.use(initReactI18next).init({
    resources: { ru: { translation: ru }, en: { translation: en } },
    lng,
    fallbackLng: "en",
    interpolation: { escapeValue: false },
  });
}
