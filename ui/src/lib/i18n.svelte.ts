// Lightweight i18n: a reactive locale plus a `t(key, params)` lookup with English fallback.
// All eight OnionHop languages are bundled. RTL languages set the document direction.

import en from './locales/en.json';
import de from './locales/de.json';
import fr from './locales/fr.json';
import ru from './locales/ru.json';
import zh from './locales/zh.json';
import azb from './locales/azb.json';
import ckb from './locales/ckb.json';
import fa from './locales/fa.json';

type Dict = Record<string, unknown>;

const DICTS: Record<string, Dict> = { en, de, fr, ru, zh, azb, ckb, fa };

export interface Language {
  code: string;
  name: string;
  rtl: boolean;
}

/** Display languages (native names), matching the OnionHop set. */
export const LANGUAGES: Language[] = [
  { code: 'en', name: 'English', rtl: false },
  { code: 'de', name: 'Deutsch', rtl: false },
  { code: 'fr', name: 'Français', rtl: false },
  { code: 'ru', name: 'Русский', rtl: false },
  { code: 'zh', name: '简体中文', rtl: false },
  { code: 'azb', name: 'تۆرکجه', rtl: true },
  { code: 'ckb', name: 'کوردیی ناوەندی', rtl: true },
  { code: 'fa', name: 'فارسی', rtl: true }
];

const STORAGE_KEY = 'bridgehop-locale';

let current = $state('en');

export function getLocale(): string {
  return current;
}

export function isRtl(code: string = current): boolean {
  return LANGUAGES.find((l) => l.code === code)?.rtl ?? false;
}

function lookup(dict: Dict | undefined, key: string): string | undefined {
  let node: unknown = dict;
  for (const part of key.split('.')) {
    if (node && typeof node === 'object' && part in (node as Dict)) {
      node = (node as Dict)[part];
    } else {
      return undefined;
    }
  }
  return typeof node === 'string' ? node : undefined;
}

/** Translate `key`, falling back to English then the key itself; `{placeholders}` are substituted. */
export function t(key: string, params?: Record<string, string | number>): string {
  let str = lookup(DICTS[current], key) ?? lookup(DICTS.en, key) ?? key;
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      str = str.replaceAll(`{${name}}`, String(value));
    }
  }
  return str;
}

function apply() {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = current;
  document.documentElement.dir = isRtl() ? 'rtl' : 'ltr';
}

export function setLocale(code: string) {
  current = DICTS[code] ? code : 'en';
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(STORAGE_KEY, current);
  }
  apply();
}

export function initLocale() {
  let code = 'en';
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved && DICTS[saved]) code = saved;
  }
  current = code;
  apply();
}
