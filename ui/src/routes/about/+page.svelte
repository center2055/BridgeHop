<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import Icon from '$lib/Icon.svelte';
  import BrandIcon from '$lib/BrandIcon.svelte';
  import { openExternal, inTauri } from '$lib/ipc';
  import { t } from '$lib/i18n.svelte';

  // Falls back to this in the browser dev preview; in the app it's read from the build at runtime.
  let version = $state('1.1.1');
  const BTC = 'bc1q0gvnvrr0a64kpxylwgqkvlp5gt4c48jqxy9jy2';

  let copied = $state(false);

  type Release = {
    name: string | null;
    tag_name: string;
    published_at: string;
    body: string | null;
    html_url: string;
    prerelease: boolean;
  };
  let releases = $state<Release[]>([]);
  let relState = $state<'loading' | 'ok' | 'empty' | 'error'>('loading');

  async function open(url: string) {
    if (inTauri()) {
      try {
        await openExternal(url);
      } catch {
        /* ignore */
      }
    } else {
      window.open(url, '_blank');
    }
  }

  async function copyBtc() {
    try {
      await navigator.clipboard.writeText(BTC);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* ignore */
    }
  }

  onMount(async () => {
    if (inTauri()) {
      try {
        version = await getVersion();
      } catch {
        /* keep fallback */
      }
    }
    try {
      const res = await fetch('https://api.github.com/repos/center2055/BridgeHop/releases', {
        headers: { Accept: 'application/vnd.github+json' }
      });
      if (!res.ok) {
        relState = 'empty';
        return;
      }
      const data = await res.json();
      if (Array.isArray(data) && data.length > 0) {
        releases = data;
        relState = 'ok';
      } else {
        relState = 'empty';
      }
    } catch {
      relState = 'error';
    }
  });

  function fmtDate(iso: string): string {
    return new Date(iso).toLocaleDateString();
  }
</script>

<header class="page-head">
  <h1>{t('about.title')}</h1>
  <p>{t('about.subtitle')}</p>
</header>

<div class="grid">
  <div class="col">
    <!-- Hero -->
    <section class="card hero">
      <div class="logo" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="34" height="34" fill="none" stroke="#fff" stroke-width="2"
          stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 17a8 8 0 0 1 16 0" />
          <line x1="3" y1="17.5" x2="21" y2="17.5" />
          <line x1="8" y1="11" x2="8" y2="17" />
          <line x1="12" y1="9.2" x2="12" y2="17" />
          <line x1="16" y1="11" x2="16" y2="17" />
        </svg>
      </div>
      <div class="hero-text">
        <strong>BridgeHop</strong>
        <span class="version">{t('about.version', { version })}</span>
        <span class="tagline">{t('about.tagline')}</span>
      </div>
    </section>

    <!-- Links -->
    <section class="card panel">
      <h2>{t('about.links')}</h2>
      <div class="links">
        <button class="link-btn" onclick={() => open('https://github.com/center2055/BridgeHop/issues')}>
          <BrandIcon name="github" /> {t('about.linkGithub')}
        </button>
        <button class="link-btn" onclick={() => open('https://discord.gg/y3MVspPzKQ')}>
          <BrandIcon name="discord" /> {t('about.linkDiscord')}
        </button>
        <button class="link-btn" onclick={() => open('https://ko-fi.com/center2055')}>
          <BrandIcon name="kofi" /> {t('about.linkKofi')}
        </button>
        <button class="link-btn" onclick={() => open('https://t.me/centerhop')}>
          <BrandIcon name="telegram" /> {t('about.linkTelegram')}
        </button>
      </div>
    </section>

    <!-- Donate -->
    <section class="card panel">
      <h2>{t('about.donate')}</h2>
      <p class="muted">{t('about.donateIntro')}</p>
      <div class="btc">
        <span class="mono btc-addr">{BTC}</span>
        <button class="link-btn" onclick={copyBtc}>{copied ? t('about.copied') : t('about.copy')}</button>
      </div>
    </section>

    <!-- Bridge sources -->
    <section class="card panel">
      <h2>{t('about.bridgeSources')}</h2>
      <p class="muted">{t('about.bridgeSourcesIntro')}</p>
      <button class="link-btn wide" onclick={() => open('https://bridges.torproject.org')}>
        <Icon name="external" size={17} /> Tor Project BridgeDB
      </button>
      <button class="link-btn wide" onclick={() => open('https://github.com/center2055/OnionHop-Bridges-Collector')}>
        <BrandIcon name="github" /> OnionHop Bridges Collector
      </button>
      <p class="muted small">{t('about.collectorDerived')}</p>
      <button class="link-btn wide" onclick={() => open('https://github.com/Delta-Kronecker/Tor-Bridges-Collector')}>
        <BrandIcon name="github" /> Delta-Kronecker/Tor-Bridges-Collector
      </button>
      <p class="muted small">{t('about.regionLists')}</p>
      <button class="link-btn wide" onclick={() => open('https://github.com/igareck/vpn-configs-for-russia')}>
        <BrandIcon name="github" /> igareck/vpn-configs-for-russia
      </button>
      <p class="muted small">{t('about.frontedNote')}</p>
    </section>
  </div>

  <div class="col">
    <!-- Changelog / releases -->
    <section class="card panel">
      <h2>{t('about.changelog')}</h2>
      {#if relState === 'loading'}
        <p class="muted">{t('common.loading')}</p>
      {:else if relState === 'ok'}
        {#each releases as rel (rel.tag_name)}
          <details class="rel">
            <summary>
              <span class="rel-title">{rel.name || rel.tag_name}</span>
              <span class="rel-meta">{rel.tag_name} &middot; {fmtDate(rel.published_at)}</span>
            </summary>
            <pre class="rel-body">{rel.body || 'No notes.'}</pre>
          </details>
        {/each}
      {:else}
        <p class="muted">
          {relState === 'error' ? t('about.loadError') : t('about.noReleases')}
        </p>
        <button class="link-btn" onclick={() => open('https://github.com/center2055/BridgeHop/releases')}>
          <BrandIcon name="github" /> {t('about.viewReleases')}
        </button>
      {/if}
    </section>

    <section class="card panel">
      <h2>{t('about.license')}</h2>
      <p class="muted">{t('about.licenseNote')}</p>
      <button class="link-btn" onclick={() => open('https://www.gnu.org/licenses/gpl-3.0.html')}>
        <Icon name="external" size={17} /> {t('about.readLicense')}
      </button>
    </section>
  </div>
</div>

<style>
  .page-head h1 {
    font-size: 26px;
  }
  .page-head p {
    margin: 4px 0 20px;
    color: var(--text-muted);
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
    align-items: start;
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .panel {
    padding: 18px 20px;
  }
  .panel h2 {
    font-size: 15px;
    margin-bottom: 10px;
  }
  .muted {
    color: var(--text-muted);
    line-height: 1.6;
    margin: 6px 0;
  }
  .muted.small {
    font-size: 12.5px;
    margin: 12px 0 4px;
  }

  .hero {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 20px;
  }
  .logo {
    width: 64px;
    height: 64px;
    flex-shrink: 0;
    border-radius: 16px;
    display: grid;
    place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: var(--shadow);
  }
  .hero-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .hero-text strong {
    font-size: 22px;
    font-weight: 700;
  }
  .version {
    font-size: 12.5px;
    color: var(--text-subtle);
  }
  .tagline {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.5;
    margin-top: 4px;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .link-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding: 0 14px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
    transition: background 0.12s ease;
  }
  .link-btn:hover {
    background: var(--surface-hover);
  }
  .link-btn.wide {
    width: 100%;
    justify-content: flex-start;
    margin-bottom: 2px;
  }

  .btc {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
    margin-top: 6px;
  }
  .btc-addr {
    flex: 1;
    font-size: 12px;
    word-break: break-all;
    color: var(--text-muted);
  }

  .rel {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
    margin-bottom: 10px;
    background: var(--surface-2);
  }
  .rel summary {
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .rel-title {
    font-weight: 600;
    font-size: 13.5px;
  }
  .rel-meta {
    font-size: 12px;
    color: var(--text-subtle);
  }
  .rel-body {
    margin: 10px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 12.5px;
    color: var(--text-muted);
    font-family: ui-sans-serif, system-ui, sans-serif;
  }

  @media (max-width: 720px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
