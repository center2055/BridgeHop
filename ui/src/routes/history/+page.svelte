<script lang="ts">
  import { onMount } from 'svelte';
  import { listRuns, inTauri, type RunSummary } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';

  let runs = $state<RunSummary[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    if (!inTauri()) {
      loading = false;
      return;
    }
    try {
      runs = await listRuns(100);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  function fmtDate(unix: number): string {
    return new Date(unix * 1000).toLocaleString();
  }

  function working(r: RunSummary): number {
    return r.reachable + r.slow + r.fronted;
  }
</script>

<header class="page-head">
  <h1>{t('history.title')}</h1>
  <p>{t('history.subtitle')}</p>
</header>

{#if error}
  <div class="placeholder card">{error}</div>
{:else if loading}
  <div class="placeholder card">{t('common.loading')}</div>
{:else if runs.length === 0}
  <div class="placeholder card">{t('history.empty')}</div>
{:else}
  <section class="card table-card">
    <table>
      <thead>
        <tr>
          <th>{t('history.col.when')}</th>
          <th class="hide-sm">{t('history.col.source')}</th>
          <th class="num hide-sm">{t('history.col.total')}</th>
          <th class="num">{t('history.col.working')}</th>
          <th class="num">{t('history.col.down')}</th>
          <th class="num hide-sm">{t('history.col.skipped')}</th>
        </tr>
      </thead>
      <tbody>
        {#each runs as r (r.id)}
          <tr>
            <td>{fmtDate(r.started_unix)}</td>
            <td class="mono source hide-sm">{r.source}</td>
            <td class="num hide-sm">{r.total}</td>
            <td class="num ok">{working(r)}</td>
            <td class="num down">{r.unreachable}</td>
            <td class="num muted hide-sm">{r.unparsed}</td>
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
    width: 90px;
  }
  tbody td {
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  tbody tr:hover {
    background: var(--surface-2);
  }
  .source {
    color: var(--text-muted);
    max-width: 360px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ok {
    color: var(--ok);
    font-weight: 600;
  }
  .down {
    color: var(--down);
  }
  .muted {
    color: var(--text-subtle);
  }

  @media (max-width: 720px) {
    .hide-sm {
      display: none;
    }
  }
</style>
