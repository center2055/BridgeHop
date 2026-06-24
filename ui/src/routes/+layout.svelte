<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from '$lib/Icon.svelte';
  import { t, initLocale } from '$lib/i18n.svelte';
  import { initTheme } from '$lib/theme.svelte';
  import { inTauri, openExternal } from '$lib/ipc';
  import {
    initAutoUpdate,
    getAutoUpdate,
    checkForUpdate,
    dismissedVersion,
    dismissUpdate,
    type UpdateInfo
  } from '$lib/update.svelte';

  let { children } = $props();

  // Set when the launch check finds a newer release the user hasn't already skipped.
  let update = $state<UpdateInfo | null>(null);

  async function openUpdate() {
    if (!update) return;
    try {
      await openExternal(update.url);
    } catch {
      /* ignore */
    }
    update = null;
  }

  function laterUpdate() {
    if (update) dismissUpdate(update.version);
    update = null;
  }

  const nav = [
    { href: '/', labelKey: 'nav.scan', icon: 'scan' },
    { href: '/library', labelKey: 'nav.library', icon: 'library' },
    { href: '/history', labelKey: 'nav.history', icon: 'history' },
    { href: '/settings', labelKey: 'nav.settings', icon: 'settings' },
    { href: '/about', labelKey: 'nav.about', icon: 'info' }
  ];

  onMount(() => {
    initLocale();
    initTheme();
    initAutoUpdate();

    // The window starts hidden (no blank/white flash); reveal it now that the UI is rendered.
    if (inTauri()) {
      getCurrentWindow()
        .show()
        .catch(() => {});

      // Notify (once per version) when a newer release is available, if the user hasn't opted out.
      if (getAutoUpdate()) {
        checkForUpdate()
          .then((u) => {
            if (u && u.version !== dismissedVersion()) update = u;
          })
          .catch(() => {});
      }
    }

    // Suppress the webview's default right-click menu (Back / Reload / Save as / Print),
    // while keeping it on editable fields so paste still works in the bridge input.
    const onContextMenu = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target?.closest('input, textarea, [contenteditable="true"]')) {
        e.preventDefault();
      }
    };
    window.addEventListener('contextmenu', onContextMenu);
    return () => window.removeEventListener('contextmenu', onContextMenu);
  });

  function isActive(href: string): boolean {
    return href === '/' ? $page.url.pathname === '/' : $page.url.pathname.startsWith(href);
  }
</script>

<div class="app">
  <aside class="sidebar">
    <nav>
      {#each nav as item (item.href)}
        <a href={item.href} class="nav-link" class:active={isActive(item.href)}>
          <Icon name={item.icon} />
          <span>{t(item.labelKey)}</span>
        </a>
      {/each}
    </nav>
  </aside>

  <main class="content">
    {@render children()}
  </main>
</div>

{#if update}
  <div class="qr-overlay">
    <div class="update-modal">
      <h2>{t('update.title', { version: update.version })}</h2>
      <p>{t('update.body')}</p>
      {#if update.notes}
        <pre class="update-notes">{update.notes}</pre>
      {/if}
      <div class="update-actions">
        <button class="btn btn-primary" onclick={openUpdate}>{t('update.download')}</button>
        <button class="btn" onclick={laterUpdate}>{t('update.later')}</button>
      </div>
    </div>
  </div>
{/if}

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') laterUpdate();
  }} />

<style>
  .qr-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: grid;
    place-items: center;
    z-index: 50;
    padding: 16px;
  }
  .update-modal {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 22px;
    max-width: 460px;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .update-modal h2 {
    font-size: 16px;
  }
  .update-modal p {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--text-muted);
  }
  .update-notes {
    margin: 0;
    max-height: 240px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-muted);
    font-family: ui-sans-serif, system-ui, sans-serif;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
  }
  .update-actions {
    display: flex;
    gap: 8px;
    margin-top: 6px;
    flex-wrap: wrap;
  }

  .app {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100vh;
    /* dvh = the *visible* viewport, so the mobile bottom nav isn't pushed off-screen. */
    height: 100dvh;
    overflow: hidden;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    padding: 18px 14px;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 4px;
  }
  .nav-link {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 11px;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    text-decoration: none;
    font-weight: 600;
    font-size: 13.5px;
    transition: background 0.12s ease, color 0.12s ease;
  }
  .nav-link:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .nav-link.active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .content {
    overflow-y: auto;
    padding: 28px 32px 40px;
  }

  /* Mobile / narrow window: sidebar becomes a bottom navigation bar. */
  @media (max-width: 720px) {
    .app {
      grid-template-columns: 1fr;
      grid-template-rows: 1fr auto;
    }
    .content {
      order: 1;
      padding: 18px 16px;
    }
    .sidebar {
      order: 2;
      flex-direction: row;
      align-items: stretch;
      border-right: none;
      border-top: 1px solid var(--border);
      /* Extra bottom padding clears the Android gesture/system nav bar so the tabs are tappable. */
      padding: 4px 4px calc(4px + env(safe-area-inset-bottom, 0px));
    }
    nav {
      flex-direction: row;
      flex: 1;
      gap: 2px;
      margin: 0;
    }
    .nav-link {
      flex: 1;
      flex-direction: column;
      gap: 3px;
      justify-content: center;
      padding: 7px 4px;
      font-size: 10.5px;
    }
  }
</style>
