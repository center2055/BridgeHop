<script lang="ts">
  import { t, LANGUAGES, getLocale, setLocale } from '$lib/i18n.svelte';
  import { getTheme, setTheme } from '$lib/theme.svelte';
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
</style>
