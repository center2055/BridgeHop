// Lightweight update notifier: checks GitHub Releases for a newer version and lets the user open
// the download page. Cross-platform (desktop + Android), no signing key required. The actual
// install is a click on the release page.

import { getVersion } from '@tauri-apps/api/app';

const PREF_KEY = 'bridgehop-autoupdate';
const DISMISS_KEY = 'bridgehop-update-dismissed';
const RELEASES_API = 'https://api.github.com/repos/center2055/BridgeHop/releases';
const RELEASES_PAGE = 'https://github.com/center2055/BridgeHop/releases/latest';

export interface UpdateInfo {
  version: string;
  notes: string;
  url: string;
}

// Whether to check for updates on launch. Defaults on; persisted to localStorage.
let autoUpdate = $state(true);

export function getAutoUpdate(): boolean {
  return autoUpdate;
}

export function setAutoUpdate(value: boolean): void {
  autoUpdate = value;
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(PREF_KEY, value ? '1' : '0');
  }
}

export function initAutoUpdate(): void {
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem(PREF_KEY);
    autoUpdate = saved === null ? true : saved === '1';
  }
}

/** Compare dotted versions; true when `latest` is strictly newer than `current`. */
function isNewer(latest: string, current: string): boolean {
  const a = latest.replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
  const b = current.replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

/**
 * Check GitHub for a newer published release. Resolves to the update info, or `null` if already up
 * to date (or not running inside the app). **Throws** if the lookup itself fails (offline, GitHub
 * rate-limit, non-OK response) so callers can tell a failed check apart from "up to date" — the
 * launch check swallows the error (stays silent); "Check now" surfaces it.
 */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  let current: string;
  try {
    current = await getVersion();
  } catch {
    return null; // not running inside the app (e.g. browser dev preview)
  }
  const res = await fetch(RELEASES_API, { headers: { Accept: 'application/vnd.github+json' } });
  if (!res.ok) throw new Error(`GitHub releases request failed (${res.status})`);
  const data: unknown = await res.json();
  if (!Array.isArray(data)) return null;
  // Pick the newest release by semver, not by array position: GitHub orders the array by the
  // release's creation date, which can diverge from version order (backports, re-created releases).
  const candidates = data.filter(
    (r) =>
      r &&
      typeof r === 'object' &&
      !(r as any).draft &&
      !(r as any).prerelease &&
      typeof (r as any).tag_name === 'string'
  ) as Array<{ tag_name: string; body?: string; html_url?: string }>;
  if (candidates.length === 0) return null;
  const latest = candidates.reduce((best, r) => (isNewer(r.tag_name, best.tag_name) ? r : best));
  if (!isNewer(latest.tag_name, current)) return null;
  return {
    version: latest.tag_name.replace(/^v/, ''),
    notes: latest.body ?? '',
    url: latest.html_url ?? RELEASES_PAGE
  };
}

/** The version the user last chose to skip ("Later"), so we don't nag every launch. */
export function dismissedVersion(): string | null {
  return typeof localStorage !== 'undefined' ? localStorage.getItem(DISMISS_KEY) : null;
}

export function dismissUpdate(version: string): void {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(DISMISS_KEY, version);
  }
}
