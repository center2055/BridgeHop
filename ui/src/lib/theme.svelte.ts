// Reactive light/dark theme, persisted to localStorage and applied via data-theme on <html>.

type Theme = 'light' | 'dark';

const STORAGE_KEY = 'bridgehop-theme';

let theme = $state<Theme>('dark');

export function getTheme(): Theme {
  return theme;
}

function apply() {
  if (typeof document !== 'undefined') {
    document.documentElement.setAttribute('data-theme', theme);
  }
}

export function setTheme(value: string) {
  theme = value === 'light' ? 'light' : 'dark';
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(STORAGE_KEY, theme);
  }
  apply();
}

export function initTheme() {
  let value: Theme = 'dark';
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'light' || saved === 'dark') {
      value = saved;
    } else if (typeof window !== 'undefined' && !window.matchMedia('(prefers-color-scheme: dark)').matches) {
      value = 'light';
    }
  }
  theme = value;
  apply();
}
