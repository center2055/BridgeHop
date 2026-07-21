<script lang="ts">
  import { onMount } from 'svelte';
  import { reliability, slipnetUri, clearHistory, inTauri, type Reliability } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';

  let rows = $state<Reliability[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    if (!inTauri()) {
      loading = false;
      return;
    }
    try {
      rows = await reliability(300);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  function pct(uptime: number): number {
    return Math.round(uptime * 100);
  }

  function shortRaw(raw: string): string {
    return raw.length > 64 ? raw.slice(0, 64) + '...' : raw;
  }

  function fmtAvg(avg: number | null): string {
    return avg == null ? '-' : `${Math.round(avg)} ms`;
  }

  function fmtLast(unix: number): string {
    return new Date(unix * 1000).toLocaleDateString();
  }

  let copied = $state<{ id: string; kind: 'raw' | 'slipnet' } | null>(null);
  function flash(id: string, kind: 'raw' | 'slipnet') {
    copied = { id, kind };
    setTimeout(() => {
      if (copied?.id === id && copied.kind === kind) copied = null;
    }, 1200);
  }
  async function copyRaw(r: Reliability) {
    try {
      await navigator.clipboard.writeText(r.raw);
      flash(r.bridge_id, 'raw');
    } catch (e) {
      error = String(e);
    }
  }
  async function copySlipnet(r: Reliability) {
    try {
      const uri = await slipnetUri(r.raw);
      await navigator.clipboard.writeText(uri);
      flash(r.bridge_id, 'slipnet');
    } catch (e) {
      error = String(e);
    }
  }

  // Narrow the table by transport and minimum uptime; Copy grabs the filtered raw lines.
  let transportFilter = $state('all');
  let minUptime = $state(0);
  const transports = $derived([...new Set(rows.map((r) => r.transport))].sort());
  const filtered = $derived(
    rows.filter(
      (r) => (transportFilter === 'all' || r.transport === transportFilter) && r.uptime >= minUptime
    )
  );
  let copiedAll = $state(false);
  async function copyFiltered() {
    if (filtered.length === 0) return;
    try {
      await navigator.clipboard.writeText(filtered.map((r) => r.raw).join('\n'));
      copiedAll = true;
      setTimeout(() => (copiedAll = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  // Two-tap clear: first tap arms it, second within 3s wipes the recorded scans.
  let confirming = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | null = null;
  async function clearLibrary() {
    if (!confirming) {
      confirming = true;
      confirmTimer = setTimeout(() => (confirming = false), 3000);
      return;
    }
    if (confirmTimer) clearTimeout(confirmTimer);
    confirming = false;
    try {
      await clearHistory();
      rows = [];
    } catch (e) {
      error = String(e);
    }
  }
</script>

<header class="page-head">
  <div class="head-text">
    <h1>{t('library.title')}</h1>
    <p>{t('library.subtitle')}</p>
  </div>
  {#if rows.length > 0}
    <button class="clear-btn" class:confirming onclick={clearLibrary}>
      {confirming ? t('library.clearConfirm') : t('library.clear')}
    </button>
  {/if}
</header>

{#if error}
  <div class="placeholder card">{error}</div>
{:else if loading}
  <div class="placeholder card">{t('common.loading')}</div>
{:else if rows.length === 0}
  <div class="placeholder card">{t('library.empty')}</div>
{:else}
  <div class="filters">
    <select class="input" bind:value={transportFilter} aria-label={t('library.col.transport')}>
      <option value="all">{t('library.allTransports')}</option>
      {#each transports as tr (tr)}
        <option value={tr}>{tr}</option>
      {/each}
    </select>
    <select class="input" bind:value={minUptime} aria-label={t('library.col.uptime')}>
      <option value={0}>{t('library.uptimeAny')}</option>
      <option value={0.5}>&ge; 50%</option>
      <option value={0.75}>&ge; 75%</option>
      <option value={0.9}>&ge; 90%</option>
      <option value={1}>100%</option>
    </select>
    <button class="copy-btn" onclick={copyFiltered} disabled={filtered.length === 0}>
      {copiedAll ? t('library.copiedAll') : t('library.copyAll', { count: filtered.length })}
    </button>
  </div>
  {#if filtered.length === 0}
    <div class="placeholder card">{t('library.noMatch')}</div>
  {:else}
  <section class="card table-card">
    <table>
      <thead>
        <tr>
          <th>{t('library.col.transport')}</th>
          <th>{t('library.col.bridge')}</th>
          <th class="uptime-col">{t('library.col.uptime')}</th>
          <th class="num hide-sm">{t('library.col.probes')}</th>
          <th class="num hide-sm">{t('library.col.avg')}</th>
          <th class="num hide-sm">{t('library.col.lastSeen')}</th>
          <th class="col-actions">{t('scan.col.actions')}</th>
        </tr>
      </thead>
      <tbody>
        {#each filtered as r (r.bridge_id)}
          <tr>
            <td><span class="chip">{r.transport}</span></td>
            <td class="mono raw" title={r.raw}>{shortRaw(r.raw)}</td>
            <td class="uptime-col">
              <div class="uptime">
                <div class="bar"><div class="fill" style:width="{pct(r.uptime)}%"></div></div>
                <span class="pct">{pct(r.uptime)}%</span>
              </div>
              <!-- The dedicated AVG column is hidden on phones; surface the ping here instead. -->
              <span class="ping-sm">{fmtAvg(r.avg_ms)}</span>
            </td>
            <td class="num hide-sm">{r.probes}</td>
            <td class="num hide-sm">{fmtAvg(r.avg_ms)}</td>
            <td class="num muted hide-sm">{fmtLast(r.last_unix)}</td>
            <td class="col-actions">
              <div class="row-actions">
                <button class="copy-btn" title={r.raw} onclick={() => copyRaw(r)}>
                  {copied?.id === r.bridge_id && copied.kind === 'raw' ? '✓' : t('scan.rowCopy')}
                </button>
                <button class="copy-btn" title="Copy as SlipNet config" onclick={() => copySlipnet(r)}>
                  {copied?.id === r.bridge_id && copied.kind === 'slipnet' ? '✓' : 'SlipNet'}
                </button>
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
  {/if}
{/if}

<style>
  .page-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }
  .page-head h1 {
    font-size: 26px;
  }
  .page-head p {
    margin: 4px 0 20px;
    color: var(--text-muted);
  }
  .clear-btn {
    flex-shrink: 0;
    margin-top: 4px;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-muted);
    border-radius: 8px;
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .clear-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .clear-btn.confirming {
    border-color: var(--down);
    background: var(--down-soft);
    color: var(--down);
  }
  .placeholder {
    padding: 24px;
    color: var(--text-subtle);
  }
  .filters {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 14px;
  }
  /* The shared .input stretches full-width; the filter dropdowns size to their content. */
  .filters .input {
    width: auto;
    max-width: 190px;
  }
  .copy-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .table-card {
    overflow: hidden;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    /* Fixed layout pins the table to 100% width at any window size, so the actions column is
       never pushed off the right edge; the bridge text truncates instead. */
    table-layout: fixed;
  }
  thead th:first-child,
  tbody td:first-child {
    width: 96px;
  }
  thead th {
    text-align: left;
    font-size: 11.5px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-subtle);
    padding: 12px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }
  th.num,
  td.num {
    text-align: right;
    width: 74px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .uptime-col {
    width: 140px;
  }
  tbody td {
    padding: 10px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    vertical-align: middle;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tbody tr:hover {
    background: var(--surface-2);
  }
  .raw {
    color: var(--text-muted);
    max-width: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .uptime {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  .bar {
    flex: 1;
    height: 7px;
    border-radius: 999px;
    background: var(--surface-hover);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
  }
  .pct {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    width: 34px;
    text-align: right;
  }
  /* Shown only on phones, where the dedicated AVG (ping) column is hidden. */
  .ping-sm {
    display: none;
  }
  .muted {
    color: var(--text-subtle);
  }
  .col-actions {
    width: 120px;
    text-align: right;
  }
  .row-actions {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .copy-btn {
    flex: 1;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-muted);
    border-radius: 7px;
    padding: 6px 10px;
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }
  .copy-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  @media (max-width: 720px) {
    .hide-sm {
      display: none;
    }
    /* Fixed layout pins the table to 100% width so the action column can't be clipped off-screen;
       the bridge text truncates instead. */
    table {
      table-layout: fixed;
    }
    thead th,
    tbody td {
      padding: 8px 5px;
    }
    thead th:first-child,
    tbody td:first-child {
      width: 66px;
    }
    /* On phones the bar would squeeze the actions column off-screen; show the % and ping instead. */
    .bar {
      display: none;
    }
    .pct {
      width: auto;
    }
    .ping-sm {
      display: block;
      margin-top: 2px;
      font-size: 11px;
      font-weight: 600;
      color: var(--text-subtle);
      white-space: nowrap;
    }
    .uptime-col {
      width: 64px;
    }
    .col-actions {
      width: 104px;
    }
    .raw {
      max-width: none;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .row-actions {
      flex-direction: column;
      align-items: stretch;
      gap: 4px;
    }
    .copy-btn {
      padding: 6px 6px;
    }
  }
</style>
