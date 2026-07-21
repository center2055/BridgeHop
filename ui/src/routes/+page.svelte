<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import {
    startScan,
    cancelScan,
    onScanProgress,
    fetchBridges,
    exportBridges,
    saveTextFile,
    importBridgesFile,
    deepStatus,
    slipnetUri,
    openExternal,
    openPtDir,
    inTauri,
    isMobile,
    SOURCE_TRANSPORTS,
    CATEGORIES,
    type ScanResult,
    type Reachability,
    type Category,
    type ExportFormat
  } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';

  // Persist the scan across navigation: leaving for Library and coming back would otherwise
  // remount this page and wipe the results. We stash the input/results/source in sessionStorage.
  const SCAN_KEY = 'bridgehop-scan';
  function loadSaved(): { linesText?: string; results?: ScanResult[]; loadedSource?: string | null } | null {
    if (typeof sessionStorage === 'undefined') return null;
    try {
      return JSON.parse(sessionStorage.getItem(SCAN_KEY) ?? 'null');
    } catch {
      return null;
    }
  }
  const saved = loadSaved();

  let linesText = $state(saved?.linesText ?? '');
  let workers = $state(16);
  let timeoutMs = $state(3000);
  let deepVerify = $state(false);
  let scanning = $state(false);
  let results = $state<ScanResult[]>(saved?.results ?? []);
  let error = $state<string | null>(null);
  let unlisten: (() => void) | null = null;

  // Source loading
  let sourceTransport = $state('all');
  let sourceCategory = $state<Category>('tested');
  let sourceIpv6 = $state(false);
  let loadingSource = $state(false);
  let sourceInfo = $state<string | null>(null);
  let loadedSource = $state<string | null>(saved?.loadedSource ?? null);

  // Deep-verify install prompt
  let deepModalOpen = $state(false);
  let deepPtDir = $state('');

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

  // Save the scan whenever it changes, so returning to this page restores it.
  $effect(() => {
    if (typeof sessionStorage === 'undefined') return;
    try {
      sessionStorage.setItem(SCAN_KEY, JSON.stringify({ linesText, results, loadedSource }));
    } catch {
      /* ignore storage quota / serialization errors */
    }
  });

  async function runScan() {
    if (scanning) return;
    const lines = linesText
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l.length > 0 && !l.startsWith('#'));
    if (lines.length === 0) {
      error = t('scan.msg.addOne');
      return;
    }
    if (!inTauri()) {
      error = t('scan.msg.desktopScan');
      return;
    }
    error = null;
    results = [];
    scanning = true;
    try {
      await startScan({ lines, workers, timeoutMs, deep: deepVerify, source: loadedSource ?? 'manual' });
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
      error = t('scan.msg.desktopSources');
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
      loadedSource = result.source;
      const cached = result.stale ? t('scan.msg.cachedSuffix') : '';
      sourceInfo = t('scan.msg.loaded', { count: result.lines.length, source: result.source, cached });
    } catch (e) {
      error = String(e);
    } finally {
      loadingSource = false;
    }
  }

  function workingRaws(): string[] {
    return results.filter((r) => r.reachability !== 'unreachable' && r.reachability !== 'unparsed').map((r) => r.raw);
  }

  async function exportFile(format: ExportFormat) {
    const working = workingRaws();
    if (working.length === 0) {
      error = t('scan.msg.noneToExport');
      return;
    }
    if (!inTauri()) {
      error = t('scan.msg.desktopSave');
      return;
    }
    try {
      const text = await exportBridges(working, format);
      const name = format === 'json' ? 'bridges.json' : format === 'torrc' ? 'bridges.torrc' : 'bridges.txt';
      const saved = await saveTextFile(name, text);
      if (saved) {
        sourceInfo = t('scan.msg.saved', { count: working.length, path: saved });
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function importFile() {
    if (!inTauri()) {
      error = t('scan.msg.desktopImport');
      return;
    }
    try {
      const lines = await importBridgesFile();
      if (lines === null) return; // user cancelled
      if (lines.length === 0) {
        error = t('scan.msg.importEmpty');
        return;
      }
      linesText = lines.join('\n');
      loadedSource = null;
      results = [];
      error = null;
      sourceInfo = t('scan.msg.imported', { count: lines.length });
    } catch (e) {
      error = String(e);
    }
  }

  async function copyRaw(raw: string) {
    try {
      await navigator.clipboard.writeText(raw);
      sourceInfo = t('scan.msg.copiedLine');
    } catch (e) {
      error = String(e);
    }
  }

  async function copySlipnet(raw: string) {
    try {
      const uri = await slipnetUri(raw);
      await navigator.clipboard.writeText(uri);
      sourceInfo = t('scan.msg.copiedLine');
    } catch (e) {
      error = String(e);
    }
  }

  // Bulk-copy every working (valid) bridge line to the clipboard.
  let copiedAll = $state(false);
  async function copyWorking() {
    const working = workingRaws();
    if (working.length === 0) return;
    try {
      await navigator.clipboard.writeText(working.join('\n'));
      copiedAll = true;
      setTimeout(() => (copiedAll = false), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  async function toggleDeep(wanted: boolean) {
    if (!wanted) {
      deepVerify = false;
      return;
    }
    if (!inTauri()) {
      deepVerify = false;
      error = t('scan.msg.desktopDeep');
      return;
    }
    try {
      const status = await deepStatus();
      deepPtDir = status.pt_dir;
      if (status.available) {
        deepVerify = true;
      } else {
        // No obfs4 client found: prompt to install it or stay off.
        deepVerify = false;
        deepModalOpen = true;
      }
    } catch (e) {
      deepVerify = false;
      error = String(e);
    }
  }

  async function installObfs4() {
    try {
      await openPtDir();
    } catch (e) {
      error = String(e);
    }
  }

  async function getTorBrowser() {
    try {
      await openExternal('https://www.torproject.org/download/');
    } catch (e) {
      error = String(e);
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

</script>

<header class="page-head">
  <div>
    <h1>{t('scan.title')}</h1>
    <p>{t('scan.subtitle')}</p>
  </div>
</header>

<section class="card controls">
  <div class="source-row">
    <div class="field">
      <label for="src-transport">{t('scan.source')}</label>
      <select id="src-transport" class="input" bind:value={sourceTransport}>
        {#each SOURCE_TRANSPORTS as t (t)}
          <option value={t}>{t}</option>
        {/each}
      </select>
    </div>
    <div class="field">
      <label for="src-category">{t('scan.category')}</label>
      <select id="src-category" class="input" bind:value={sourceCategory}>
        {#each CATEGORIES as c (c.value)}
          <option value={c.value}>{t('scan.cat.' + c.value)}</option>
        {/each}
      </select>
    </div>
    <label class="checkbox" title={t('scan.ipv6Hint')}>
      <input type="checkbox" bind:checked={sourceIpv6} /> {t('scan.ipv6')}
    </label>
    <button class="btn" onclick={loadFromSource} disabled={loadingSource}>
      <Icon name="library" size={15} />
      {loadingSource ? t('common.loading') : t('scan.loadBridges')}
    </button>
    {#if !isMobile()}
      <button class="btn" onclick={importFile}>
        <Icon name="external" size={15} /> {t('scan.importFile')}
      </button>
    {/if}
    {#if sourceInfo}
      <span class="source-info">{sourceInfo}</span>
    {/if}
  </div>

  <div class="controls-grid">
    <div class="field bridges-field">
      <label for="bridges">{t('scan.bridgeLines')}</label>
      <textarea
        id="bridges"
        class="textarea"
        rows="9"
        spellcheck="false"
        placeholder={t('scan.bridgePlaceholder')}
        bind:value={linesText}
        oninput={() => (loadedSource = null)}></textarea>
    </div>

    <div class="settings">
      <div class="field">
        <label for="workers">{t('scan.concurrency')}</label>
        <input id="workers" class="input" type="number" min="1" max="64" bind:value={workers} />
      </div>
      <div class="field">
        <label for="timeout">{t('scan.timeout')}</label>
        <input id="timeout" class="input" type="number" min="500" max="60000" step="500" bind:value={timeoutMs} />
      </div>
      {#if !isMobile()}
        <label class="checkbox deep-toggle" title={t('scan.deepVerifyHint')}>
          <input type="checkbox" checked={deepVerify} onchange={(e) => toggleDeep(e.currentTarget.checked)} /> {t('scan.deepVerify')}
        </label>
      {/if}
      <div class="actions">
        {#if scanning}
          <button class="btn btn-danger" onclick={stopScan}>
            <Icon name="stop" size={15} /> {t('scan.stopBtn')}
          </button>
        {:else}
          <button class="btn btn-primary" onclick={runScan}>
            <Icon name="play" size={15} /> {t('scan.scanBtn')}
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
    <div class="stat"><span class="stat-value">{summary.total}</span><span class="stat-label">{t('scan.sum.scanned')}</span></div>
    <div class="stat ok"><span class="stat-value">{summary.working}</span><span class="stat-label">{t('scan.sum.working')}</span></div>
    <div class="stat"><span class="stat-value">{summary.reachable}</span><span class="stat-label">{t('scan.sum.reachable')}</span></div>
    <div class="stat"><span class="stat-value">{summary.fronted}</span><span class="stat-label">{t('scan.sum.fronted')}</span></div>
    <div class="stat down"><span class="stat-value">{summary.unreachable}</span><span class="stat-label">{t('scan.sum.unreachable')}</span></div>
  </section>

  <div class="results-toolbar">
    <button class="btn small" onclick={copyWorking} disabled={summary.working === 0}>
      {copiedAll ? t('scan.copiedAll') : t('scan.copyWorking', { count: summary.working })}
    </button>
    {#if !isMobile()}
      <span class="toolbar-label">{t('scan.exportWorking')}</span>
      <button class="btn small" onclick={() => exportFile('plain')}>{t('scan.exportPlain')}</button>
      <button class="btn small" onclick={() => exportFile('torrc')}>{t('scan.exportTorrc')}</button>
      <button class="btn small" onclick={() => exportFile('json')}>{t('scan.exportJson')}</button>
    {/if}
  </div>

  <section class="card table-card">
    <table>
      <thead>
        <tr>
          <th class="col-ping">{t('scan.col.ping')}</th>
          <th class="hide-sm">{t('scan.col.transport')}</th>
          <th>{t('scan.col.endpoint')}</th>
          <th class="hide-sm">{t('scan.col.detail')}</th>
          <th class="col-actions">{t('scan.col.actions')}</th>
        </tr>
      </thead>
      <tbody>
        {#each results as r, i (i)}
          <tr>
            <td class="col-ping">
              <span class={badgeClass(r.reachability)} title={r.detail}>
                {r.ping_ms != null ? r.ping_ms : '—'}
              </span>
            </td>
            <td class="hide-sm"><span class="chip">{r.transport}</span></td>
            <td class="mono endpoint">{r.probed_host}:{r.probed_port}</td>
            <td class="detail hide-sm">
              {r.detail}
              {#if r.deep}
                <span class="deep-badge" class:ok={r.deep.ok} title={r.deep.detail}>
                  {r.deep.ok ? 'deep ✓' : 'deep ✗'}
                </span>
              {/if}
            </td>
            <td class="col-actions">
              <div class="row-actions">
                <button class="copy-btn" title={t('scan.rowCopyTitle')} onclick={() => copyRaw(r.raw)}>
                  {t('scan.rowCopy')}
                </button>
                <button class="copy-btn" title="Copy as SlipNet config" onclick={() => copySlipnet(r.raw)}>
                  SlipNet
                </button>
              </div>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
{:else}
  <section class="empty">
    <Icon name="scan" size={26} />
    <p>{t('scan.emptyHint')}</p>
  </section>
{/if}

{#if deepModalOpen}
  <div class="qr-overlay">
    <div class="deep-modal">
      <h2>{t('scan.deep.title')}</h2>
      <p>{t('scan.deep.body')}</p>
      <p class="muted">{t('scan.deep.lookHere')}</p>
      <p class="mono deep-path">{deepPtDir}</p>
      <div class="deep-actions">
        <button class="btn btn-primary" onclick={installObfs4}>{t('scan.deep.openFolder')}</button>
        <button class="btn" onclick={getTorBrowser}>{t('scan.deep.getTor')}</button>
        <button class="btn" onclick={() => (deepModalOpen = false)}>{t('scan.deep.notNow')}</button>
      </div>
    </div>
  </div>
{/if}

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      deepModalOpen = false;
    }
  }} />

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
  .col-ping {
    width: 84px;
  }
  .endpoint {
    color: var(--text-muted);
  }
  .detail {
    color: var(--text-subtle);
  }
  .muted {
    color: var(--text-subtle);
  }
  .deep-badge {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 5px;
    font-size: 10.5px;
    font-weight: 700;
    background: var(--down-soft);
    color: var(--down);
    white-space: nowrap;
  }
  .deep-badge.ok {
    background: var(--ok-soft);
    color: var(--ok);
  }

  .results-toolbar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 10px;
    margin: 18px 0 12px;
  }
  .toolbar-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    margin-right: 2px;
  }
  .btn.small {
    height: 32px;
    padding: 0 12px;
    font-size: 12.5px;
  }
  .col-actions {
    width: 184px;
    text-align: right;
  }
  .row-actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }
  .copy-btn {
    flex: 1;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text-muted);
    border-radius: 7px;
    padding: 7px 10px;
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
    white-space: nowrap;
  }
  .copy-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .qr-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: grid;
    place-items: center;
    z-index: 50;
  }
  .deep-modal {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 22px;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .deep-modal h2 {
    font-size: 16px;
  }
  .deep-modal p {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.55;
  }
  .deep-path {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 9px 11px;
    font-size: 12px;
    word-break: break-all;
  }
  .deep-actions {
    display: flex;
    gap: 8px;
    margin-top: 6px;
    flex-wrap: wrap;
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

  @media (max-width: 720px) {
    .page-head h1 {
      font-size: 22px;
    }
    .controls-grid {
      grid-template-columns: 1fr;
    }
    .stats {
      grid-template-columns: repeat(2, 1fr);
    }
    .hide-sm {
      display: none;
    }
    /* Fixed layout pins the table to 100% width: the action column can't be pushed off-screen,
       and the address truncates instead. */
    table {
      table-layout: fixed;
    }
    thead th,
    tbody td {
      padding: 8px 5px;
    }
    .col-ping {
      width: 58px;
    }
    .col-actions {
      width: 104px;
    }
    .endpoint {
      max-width: none;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    /* Stack Copy / SlipNet within the fixed action column so both fit. */
    .row-actions {
      flex-direction: column;
      align-items: stretch;
      gap: 5px;
    }
    .copy-btn {
      padding: 7px 6px;
    }
  }
</style>
