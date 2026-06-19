<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { t, initLocale } from '$lib/i18n.svelte';
  import { initTheme } from '$lib/theme.svelte';

  let { children } = $props();

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
    <div class="brand">
      <div class="brand-text">
        <strong>BridgeHop</strong>
        <span>{t('app.tagline')}</span>
      </div>
    </div>

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

<style>
  .app {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100vh;
    overflow: hidden;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    padding: 18px 14px;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 4px 8px 18px;
  }
  .brand-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .brand-text strong {
    font-size: 15.5px;
    font-weight: 700;
  }
  .brand-text span {
    font-size: 11.5px;
    color: var(--text-subtle);
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
      padding: 4px;
    }
    .brand {
      display: none;
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
