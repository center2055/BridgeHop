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
      deep: request.deep ?? false
    }
  });
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
