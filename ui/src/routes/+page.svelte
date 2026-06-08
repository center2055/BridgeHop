<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import {
    startScan,
    cancelScan,
    onScanProgress,
    fetchBridges,
    inTauri,
    SOURCE_TRANSPORTS,
    CATEGORIES,
    type ScanResult,
    type Reachability,
    type Category
  } from '$lib/ipc';

  const SAMPLE = `# Paste bridge lines here (one per line). Examples:
1.1.1.1:443 0123456789ABCDEF0123456789ABCDEF01234567
obfs4 192.95.36.142:443 CDF2E852BF539B82BD10E27E9115A31734E378C2 cert=qUVQ0srL1JI/vO6V6m/24odNwesD iat-mode=1`;

  let linesText = $state(SAMPLE);
  let workers = $state(16);
  let timeoutMs = $state(3000);
  let scanning = $state(false);
  let results = $state<ScanResult[]>([]);
  let error = $state<string | null>(null);
  let unlisten: (() => void) | null = null;

  // Source loading
  let sourceTransport = $state('all');
  let sourceCategory = $state<Category>('tested');
  let sourceIpv6 = $state(false);
  let loadingSource = $state(false);
  let sourceInfo = $state<string | null>(null);

  const summary = $derived.by(() => {
    const s = { total: results.length, working: 0, reachable: 0, slow: 0, fronted: 0, unreachable: 0, unparsed: 0 };
    for (const r of results) {
      switch (r.reachability) {
        case 'reachable': s.reachable++; s.working++; break;
        case 'slow': s.slow++; s.working++; break;
        case 'fronted': s.fronted++; s.working++; break;
        case 'unreachable': s.unreachable++; break;
        default: s.unparsed++;
      }
    }
    return s;
  });

  onMount(async () => {
    if (!inTauri()) return;
    try {
      unlisten = await onScanProgress((r) => {
        results = [...results, r];
      });
    } catch (e) {
      error = String(e);
    }
  });

  onDestroy(() => unlisten?.());

  async function runScan() {
    if (scanning) return;
    const lines = linesText
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith('#'));
    if (lines.length === 0) {
      error = 'Add at least one bridge line.';
      return;
    }
    if (!inTauri()) {
      error = 'Scanning is only available in the desktop app.';
      return;
    }
    error = null;
    results = [];
    scanning = true;
    try {
      await startScan({ lines, workers, timeoutMs });
    } catch (e) {
      error = String(e);
    } finally {
      scanning = false;
    }
  }

  async function stopScan() {
    try {
      await cancelScan();
    } catch (e) {
      error = String(e);
    }
  }

  async function loadFromSource() {
    if (loadingSource) return;
    if (!inTauri()) {
      error = 'Loading sources is only available in the desktop app.';
      return;
    }
    loadingSource = true;
    error = null;
    sourceInfo = null;
    try {
      const result = await fetchBridges({
        transport: sourceTransport,
        category: sourceCategory,
        ipv6: sourceIpv6
      });
      linesText = result.lines.join('\n');
      sourceInfo = `Loaded ${result.lines.length} bridge${result.lines.length === 1 ? '' : 's'} from ${result.source}`;
    } catch (e) {
      error = String(e);
    } finally {
      loadingSource = false;
    }
  }

  function badgeClass(r: Reachability): string {
    switch (r) {
      case 'reachable': return 'badge badge-ok';
      case 'slow': return 'badge badge-slow';
      case 'fronted': return 'badge badge-fronted';
      case 'unreachable': return 'badge badge-down';
      default: return 'badge badge-skip';
    }
  }

  function badgeLabel(r: Reachability): string {
    switch (r) {
      case 'reachable': return 'OK';
      case 'slow': return 'SLOW';
      case 'fronted': return 'FRONT';
      case 'unreachable': return 'DOWN';
      default: return 'SKIP';
    }
  }
</script>

<header class="page-head">
  <div>
    <h1>Scan bridges</h1>
    <p>Check whether Tor bridges are reachable from your network — all transport types.</p>
  </div>
</header>

<section class="card controls">
  <div class="source-row">
    <div class="field">
      <label for="src-transport">Source</label>
      <select id="src-transport" class="input" bind:value={sourceTransport}>
        {#each SOURCE_TRANSPORTS as t (t)}
          <option value={t}>{t}</option>
        {/each}
      </select>
    </div>
    <div class="field">
      <label for="src-category">Category</label>
      <select id="src-category" class="input" bind:value={sourceCategory}>
        {#each CATEGORIES as c (c.value)}
          <option value={c.value}>{c.label}</option>
        {/each}
      </select>
    </div>
    <label class="checkbox" title="Fetch the IPv6 list (collector transports only)">
      <input type="checkbox" bind:checked={sourceIpv6} /> IPv6
    </label>
    <button class="btn" onclick={loadFromSource} disabled={loadingSource}>
      <Icon name="library" size={15} />
      {loadingSource ? 'Loading…' : 'Load bridges'}
    </button>
    {#if sourceInfo}
      <span class="source-info">{sourceInfo}</span>
    {/if}
  </div>

  <div class="controls-grid">
    <div class="field bridges-field">
      <label for="bridges">Bridge lines</label>
      <textarea id="bridges" class="textarea" rows="9" spellcheck="false" bind:value={linesText}></textarea>
    </div>

    <div class="settings">
      <div class="field">
        <label for="workers">Concurrency</label>
        <input id="workers" class="input" type="number" min="1" max="64" bind:value={workers} />
      </div>
      <div class="field">
        <label for="timeout">Timeout (ms)</label>
        <input id="timeout" class="input" type="number" min="500" max="60000" step="500" bind:value={timeoutMs} />
      </div>
      <div class="actions">
        {#if scanning}
          <button class="btn btn-danger" onclick={stopScan}>
            <Icon name="stop" size={15} /> Stop
          </button>
        {:else}
          <button class="btn btn-primary" onclick={runScan}>
            <Icon name="play" size={15} /> Scan
          </button>
        {/if}
      </div>
    </div>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}
</section>

{#if results.length > 0}
  <section class="stats">
    <div class="stat"><span class="stat-value">{summary.total}</span><span class="stat-label">scanned</span></div>
    <div class="stat ok"><span class="stat-value">{summary.working}</span><span class="stat-label">working</span></div>
    <div class="stat"><span class="stat-value">{summary.reachable}</span><span class="stat-label">reachable</span></div>
    <div class="stat"><span class="stat-value">{summary.fronted}</span><span class="stat-label">fronted</span></div>
    <div class="stat down"><span class="stat-value">{summary.unreachable}</span><span class="stat-label">unreachable</span></div>
  </section>

  <section class="card table-card">
    <table>
      <thead>
        <tr>
          <th class="col-status">Status</th>
          <th class="col-ping">Ping</th>
          <th>Transport</th>
          <th>Endpoint</th>
          <th>Detail</th>
        </tr>
      </thead>
      <tbody>
        {#each results as r (r.bridge_id + r.probed_host + r.probed_port)}
          <tr>
            <td><span class={badgeClass(r.reachability)}>{badgeLabel(r.reachability)}</span></td>
            <td class="col-ping mono">{r.ping_ms != null ? `${r.ping_ms} ms` : '—'}</td>
            <td><span class="chip">{r.transport}</span></td>
            <td class="mono endpoint">{r.probed_host}:{r.probed_port}</td>
            <td class="detail">{r.detail}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{:else}
  <section class="empty">
    <Icon name="scan" size={26} />
    <p>Paste bridge lines above and hit <strong>Scan</strong> to see reachability results.</p>
  </section>
{/if}

<style>
  .page-head {
    margin-bottom: 20px;
  }
  .page-head h1 {
    font-size: 26px;
  }
  .page-head p {
    margin: 4px 0 0;
    color: var(--text-muted);
  }

  .controls {
    padding: 18px;
  }
  .source-row {
    display: flex;
    align-items: flex-end;
    gap: 12px;
    flex-wrap: wrap;
    padding-bottom: 16px;
    margin-bottom: 16px;
    border-bottom: 1px solid var(--border);
  }
  .source-row .field {
    min-width: 150px;
  }
  .source-row select.input {
    height: 38px;
  }
  .checkbox {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 38px;
    font-size: 13px;
    color: var(--text-muted);
    font-weight: 600;
    cursor: pointer;
  }
  .source-info {
    font-size: 12.5px;
    color: var(--text-subtle);
    margin-left: auto;
    align-self: center;
  }
  .controls-grid {
    display: grid;
    grid-template-columns: 1fr 220px;
    gap: 18px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .actions {
    margin-top: auto;
  }
  .actions .btn {
    width: 100%;
    justify-content: center;
    height: 42px;
  }
  .error {
    margin: 14px 0 0;
    color: var(--down);
    font-size: 13px;
    font-weight: 600;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 12px;
    margin: 18px 0;
  }
  .stat {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
  }
  .stat-value {
    font-size: 22px;
    font-weight: 700;
  }
  .stat-label {
    font-size: 11.5px;
    color: var(--text-subtle);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .stat.ok .stat-value {
    color: var(--ok);
  }
  .stat.down .stat-value {
    color: var(--down);
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
  .col-status {
    width: 86px;
  }
  .col-ping {
    width: 92px;
    color: var(--text-muted);
  }
  .endpoint {
    color: var(--text-muted);
  }
  .detail {
    color: var(--text-subtle);
  }

  .empty {
    margin-top: 60px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    color: var(--text-subtle);
  }
  .empty p {
    margin: 0;
  }
</style>
