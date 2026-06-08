<script lang="ts">
  import { onMount } from 'svelte';
  import { reliability, inTauri, type Reliability } from '$lib/ipc';

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
    return raw.length > 64 ? raw.slice(0, 64) + '…' : raw;
  }

  function fmtAvg(avg: number | null): string {
    return avg == null ? '—' : `${Math.round(avg)} ms`;
  }

  function fmtLast(unix: number): string {
    return new Date(unix * 1000).toLocaleDateString();
  }
</script>

<header class="page-head">
  <h1>Library</h1>
  <p>Bridges you've scanned, ranked by reliability across every recorded run.</p>
</header>

{#if error}
  <div class="placeholder card">{error}</div>
{:else if loading}
  <div class="placeholder card">Loading…</div>
{:else if rows.length === 0}
  <div class="placeholder card">
    No bridges yet. Scan some bridges and they'll appear here with reliability stats.
  </div>
{:else}
  <section class="card table-card">
    <table>
      <thead>
        <tr>
          <th>Transport</th>
          <th>Bridge</th>
          <th class="uptime-col">Uptime</th>
          <th class="num">Probes</th>
          <th class="num">Avg</th>
          <th class="num">Last seen</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as r (r.bridge_id)}
          <tr>
            <td><span class="chip">{r.transport}</span></td>
            <td class="mono raw" title={r.raw}>{shortRaw(r.raw)}</td>
            <td class="uptime-col">
              <div class="uptime">
                <div class="bar"><div class="fill" style:width="{pct(r.uptime)}%"></div></div>
                <span class="pct">{pct(r.uptime)}%</span>
              </div>
            </td>
            <td class="num">{r.probes}</td>
            <td class="num">{fmtAvg(r.avg_ms)}</td>
            <td class="num muted">{fmtLast(r.last_unix)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{/if}

<style>
  .page-head h1 {
    font-size: 26px;
  }
  .page-head p {
    margin: 4px 0 20px;
    color: var(--text-muted);
  }
  .placeholder {
    padding: 24px;
    color: var(--text-subtle);
  }
  .table-card {
    overflow: hidden;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  thead th {
    text-align: left;
    font-size: 11.5px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-subtle);
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }
  th.num,
  td.num {
    text-align: right;
    width: 92px;
  }
  .uptime-col {
    width: 180px;
  }
  tbody td {
    padding: 10px 16px;
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
    max-width: 380px;
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
    background: linear-gradient(90deg, var(--accent), var(--accent-2));
  }
  .pct {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    width: 34px;
    text-align: right;
  }
  .muted {
    color: var(--text-subtle);
  }
</style>
