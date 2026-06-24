<script lang="ts">
  import { t, LANGUAGES, getLocale, setLocale } from '$lib/i18n.svelte';
  import { getTheme, setTheme } from '$lib/theme.svelte';
  import { getAutoUpdate, setAutoUpdate, checkForUpdate } from '$lib/update.svelte';
  import { openExternal } from '$lib/ipc';

  let checking = $state(false);
  let checkMsg = $state<string | null>(null);
  let foundUrl = $state<string | null>(null);

  async function checkNow() {
    checking = true;
    checkMsg = null;
    foundUrl = null;
    try {
      const u = await checkForUpdate();
      if (u) {
        checkMsg = t('settings.updateAvailable', { version: u.version });
        foundUrl = u.url;
      } else {
        checkMsg = t('settings.upToDate');
      }
    } catch {
      checkMsg = t('settings.checkFailed');
    } finally {
      checking = false;
    }
  }

  async function openDownload() {
    if (!foundUrl) return;
    try {
      await openExternal(foundUrl);
    } catch {
      /* ignore */
    }
  }
</script>

<header class="page-head">
  <h1>{t('settings.title')}</h1>
  <p>{t('settings.subtitle')}</p>
</header>

<section class="card panel">
  <h2>{t('settings.appearance')}</h2>
  <p class="muted">{t('settings.appearanceNote')}</p>
  <select class="input sel" value={getTheme()} onchange={(e) => setTheme(e.currentTarget.value)}>
    <option value="light">{t('theme.light')}</option>
    <option value="dark">{t('theme.dark')}</option>
  </select>
</section>

<section class="card panel">
  <h2>{t('settings.language')}</h2>
  <p class="muted">{t('settings.languageNote')}</p>
  <select class="input sel" value={getLocale()} onchange={(e) => setLocale(e.currentTarget.value)}>
    {#each LANGUAGES as l (l.code)}
      <option value={l.code}>{l.name}</option>
    {/each}
  </select>
</section>

<section class="card panel">
  <h2>{t('settings.updates')}</h2>
  <p class="muted">{t('settings.updatesNote')}</p>
  <label class="toggle">
    <input
      type="checkbox"
      checked={getAutoUpdate()}
      onchange={(e) => setAutoUpdate(e.currentTarget.checked)} />
    {t('settings.autoUpdate')}
  </label>
  <div class="check-row">
    <button class="btn" onclick={checkNow} disabled={checking}>
      {checking ? t('common.loading') : t('settings.checkNow')}
    </button>
    {#if checkMsg}<span class="muted">{checkMsg}</span>{/if}
    {#if foundUrl}
      <button class="btn btn-primary" onclick={openDownload}>{t('update.download')}</button>
    {/if}
  </div>
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
  .sel {
    max-width: 260px;
    margin-top: 6px;
  }
  .toggle {
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 13.5px;
    font-weight: 600;
    cursor: pointer;
    margin-top: 6px;
  }
  .toggle input {
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .check-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 14px;
  }
</style>
