<script lang="ts">
  import '../app.css';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';

  let theme = $state<'light' | 'dark'>('dark');
  let { children } = $props();

  const nav = [
    { href: '/', label: 'Scan', icon: 'scan' },
    { href: '/library', label: 'Library', icon: 'library' },
    { href: '/history', label: 'History', icon: 'history' },
    { href: '/settings', label: 'Settings', icon: 'settings' }
  ];

  onMount(() => {
    const saved = localStorage.getItem('bridgehop-theme') as 'light' | 'dark' | null;
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    theme = saved ?? (prefersDark ? 'dark' : 'light');
    applyTheme();
  });

  function applyTheme() {
    document.documentElement.setAttribute('data-theme', theme);
  }

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
    localStorage.setItem('bridgehop-theme', theme);
    applyTheme();
  }

  function isActive(href: string): boolean {
    return href === '/' ? $page.url.pathname === '/' : $page.url.pathname.startsWith(href);
  }
</script>

<div class="app">
  <aside class="sidebar">
    <div class="brand">
      <div class="logo" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#fff" stroke-width="2"
          stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 17a8 8 0 0 1 16 0" />
          <line x1="3" y1="17.5" x2="21" y2="17.5" />
          <line x1="8" y1="11" x2="8" y2="17" />
          <line x1="12" y1="9.2" x2="12" y2="17" />
          <line x1="16" y1="11" x2="16" y2="17" />
        </svg>
      </div>
      <div class="brand-text">
        <strong>BridgeHop</strong>
        <span>bridge scanner</span>
      </div>
    </div>

    <nav>
      {#each nav as item (item.href)}
        <a href={item.href} class="nav-link" class:active={isActive(item.href)}>
          <Icon name={item.icon} />
          <span>{item.label}</span>
        </a>
      {/each}
    </nav>

    <div class="sidebar-footer">
      <button class="theme-toggle" onclick={toggleTheme} title="Toggle theme">
        <Icon name={theme === 'dark' ? 'sun' : 'moon'} size={16} />
        <span>{theme === 'dark' ? 'Light' : 'Dark'} mode</span>
      </button>
    </div>
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
    padding: 6px 8px 18px;
  }
  .logo {
    width: 40px;
    height: 40px;
    border-radius: 11px;
    display: grid;
    place-items: center;
    background: linear-gradient(135deg, var(--accent), var(--accent-2));
    box-shadow: var(--shadow);
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

  .sidebar-footer {
    margin-top: auto;
  }
  .theme-toggle {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 9px 11px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
    transition: background 0.12s ease;
  }
  .theme-toggle:hover {
    background: var(--surface-hover);
    color: var(--text);
  }

  .content {
    overflow-y: auto;
    padding: 28px 32px 40px;
  }
</style>
