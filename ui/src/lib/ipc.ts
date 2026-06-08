// Typed bridge between the SvelteKit front end and the Rust (Tauri) core.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type Reachability = 'reachable' | 'slow' | 'unreachable' | 'unparsed' | 'fronted';

export interface DeepResult {
  ok: boolean;
  method: string;
  socks_ms: number | null;
  detail: string;
}

export interface GeoInfo {
  country: string | null;
  asn: number | null;
  as_org: string | null;
}

/** Mirrors `bridgehop_core::model::ScanResult`. */
export interface ScanResult {
  bridge_id: string;
  raw: string;
  transport: string;
  probed_host: string;
  probed_port: number;
  ping_ms: number | null;
  reachability: Reachability;
  detail: string;
  deep: DeepResult | null;
  geo: GeoInfo | null;
}

export interface ScanRequest {
  lines: string[];
  workers: number;
  timeoutMs: number;
  deep?: boolean;
  source?: string;
}

export interface RunSummary {
  id: number;
  started_unix: number;
  finished_unix: number;
  source: string;
  transport_filter: string;
  deep: boolean;
  total: number;
  reachable: number;
  slow: number;
  fronted: number;
  unreachable: number;
  unparsed: number;
}

export interface Reliability {
  bridge_id: string;
  raw: string;
  transport: string;
  country: string | null;
  asn: number | null;
  uptime: number;
  probes: number;
  avg_ms: number | null;
  last_unix: number;
}

export type Category = 'tested' | 'fresh72h' | 'full_archive';

export interface Selection {
  transport: string;
  category: Category;
  ipv6: boolean;
}

export interface FetchResult {
  lines: string[];
  source: string;
  stale?: boolean;
}

export const SOURCE_TRANSPORTS = [
  'all',
  'obfs4',
  'webtunnel',
  'vanilla',
  'snowflake',
  'meek-azure',
  'conjure',
  'dnstt'
] as const;

export const CATEGORIES: { value: Category; label: string }[] = [
  { value: 'tested', label: 'Tested & Active' },
  { value: 'fresh72h', label: 'Fresh (72h)' },
  { value: 'full_archive', label: 'Full Archive' }
];

/** Fetch bridge lines from a source (collector mirror or built-in defaults). */
export async function fetchBridges(selection: Selection): Promise<FetchResult> {
  return invoke<FetchResult>('fetch_bridges', { selection });
}

/** True when running inside the Tauri runtime (vs. a plain browser dev preview). */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Start a scan; resolves with the full result set when finished. */
export async function startScan(request: ScanRequest): Promise<ScanResult[]> {
  return invoke<ScanResult[]>('start_scan', {
    request: {
      lines: request.lines,
      workers: request.workers,
      timeout_ms: request.timeoutMs,
      deep: request.deep ?? false,
      source: request.source ?? null
    }
  });
}

/** List recent scan runs (newest first). */
export async function listRuns(limit = 50): Promise<RunSummary[]> {
  return invoke<RunSummary[]>('list_runs', { limit });
}

/** Per-bridge reliability leaderboard across all recorded scans. */
export async function reliability(limit = 200): Promise<Reliability[]> {
  return invoke<Reliability[]>('reliability', { limit });
}

export type ExportFormat = 'plain' | 'torrc' | 'json';

/** Render bridge lines in the given export format. */
export async function exportBridges(lines: string[], format: ExportFormat): Promise<string> {
  return invoke<string>('export_bridges', { lines, format });
}

/** Render a bridge line as an SVG QR code. */
export async function qrSvg(text: string): Promise<string> {
  return invoke<string>('qr_svg', { text });
}

/** Request cancellation of the in-flight scan. */
export async function cancelScan(): Promise<void> {
  await invoke('cancel_scan');
}

/** Subscribe to per-bridge results streamed during a scan. */
export function onScanProgress(callback: (result: ScanResult) => void): Promise<UnlistenFn> {
  return listen<ScanResult>('scan-progress', (event) => callback(event.payload));
}

/** Subscribe to scan completion (payload = number of results). */
export function onScanDone(callback: (count: number) => void): Promise<UnlistenFn> {
  return listen<number>('scan-done', (event) => callback(event.payload));
}
