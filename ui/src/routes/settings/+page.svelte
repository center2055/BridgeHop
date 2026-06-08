<script lang="ts">
  import { onMount } from 'svelte';
  import { geoStatus, inTauri, type GeoStatus } from '$lib/ipc';

  let geo = $state<GeoStatus | null>(null);

  onMount(async () => {
    if (!inTauri()) return;
    try {
      geo = await geoStatus();
    } catch {
      geo = null;
    }
  });
</script>

<header class="page-head">
  <h1>Settings</h1>
  <p>Appearance, geo data, and more.</p>
</header>

<section class="card panel">
  <h2>Appearance</h2>
  <p class="muted">Use the toggle at the bottom of the sidebar to switch between light and dark themes.</p>
</section>

<section class="card panel">
  <h2>Geo / ASN database</h2>
  {#if geo}
    <p class="status">
      <span class="dot" class:on={geo.available}></span>
      {geo.available ? 'GeoLite2 database detected — country and ASN are shown in scan results.' : 'No GeoLite2 database found.'}
    </p>
    <p class="muted">
      Country and ASN lookups use MaxMind GeoLite2 (a free MaxMind account is required to download
      it; it is not bundled). Place <code>GeoLite2-Country.mmdb</code> and/or
      <code>GeoLite2-ASN.mmdb</code> in:
    </p>
    <p class="mono path">{geo.dir}</p>
  {:else}
    <p class="muted">Geo status is only available in the desktop app.</p>
  {/if}
</section>

<style>
  .page-head h1 {
    font-size: 26px;
  }
  .page-head p {
    margin: 4px 0 20px;
    color: var(--text-muted);
  }
  .panel {
    padding: 20px 22px;
    margin-bottom: 16px;
  }
  .panel h2 {
    font-size: 15px;
    margin-bottom: 8px;
  }
  .muted {
    color: var(--text-muted);
    line-height: 1.6;
    margin: 6px 0;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    margin: 0 0 8px;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--down);
  }
  .dot.on {
    background: var(--ok);
  }
  code {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 1px 6px;
    font-size: 12.5px;
  }
  .path {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    font-size: 12.5px;
    word-break: break-all;
  }
</style>
