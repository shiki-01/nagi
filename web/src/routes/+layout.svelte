<script lang="ts">
  import '../app.css'
  import { page } from '$app/state'

  let { children } = $props()
  const nav = [
    { href: '/', label: 'Nagi について' },
    { href: '/support', label: 'サポート' },
    { href: '/custom-keyboard', label: 'カスタムキーボード' },
    { href: '/privacy', label: 'プライバシー' }
  ]
</script>

<a class="skip-link" href="#main">本文へ移動</a>

<header class="site-header">
  <div class="inner flex flex-wrap:wrap justify-content:space-between gap:8px px:16px py:8px">
    <a class="brand" href="/" aria-label="Nagi ホーム">凪 Nagi</a>
    <nav aria-label="メインメニュー">
      <ul class="nav flex gap:4px">
        {#each nav as item (item.href)}
          <li>
            <a href={item.href} aria-current={page.url.pathname.replace(/\/$/, '') === item.href.replace(/\/$/, '') ? 'page' : undefined}>
              {item.label}
            </a>
          </li>
        {/each}
      </ul>
    </nav>
  </div>
</header>

<div id="main" tabindex="-1">
  {@render children()}
</div>

<footer class="site-footer">
  <div class="inner px:16px py:24px">
    <p>Nagi は個人で開発中のオープンソースの日本語入力システムです（コードは MIT OR Apache-2.0）。</p>
    <p>
      <a href="/support">サポート</a> / <a href="/privacy">プライバシーポリシー</a> /
      <a href="https://github.com/shiki-01/nagi" rel="noopener">GitHub</a>
    </p>
  </div>
</footer>

<style>
  .site-header {
    border-bottom: 1px solid var(--line);
    background: var(--surface);
    position: sticky;
    top: 0;
    z-index: 10;
  }
  .inner {
    max-width: 1200px;
    margin: 0 auto;
    box-sizing: border-box;
    align-items: center;
  }
  .brand {
    font-weight: 700;
    font-size: 1.125rem;
    white-space: nowrap;
    color: var(--fg);
    text-decoration: none;
    padding: 6px 4px;
  }
  .nav {
    list-style: none;
    margin: 0;
    padding: 0;
    flex-wrap: wrap;
  }
  .nav a {
    display: inline-block;
    padding: 8px 10px;
    border-radius: 8px;
    color: var(--fg);
    text-decoration: none;
    font-size: 0.9375rem;
  }
  .nav a:hover {
    background: var(--surface-2);
  }
  .nav a[aria-current='page'] {
    background: var(--accent-soft);
    font-weight: 600;
  }
  .site-footer {
    border-top: 1px solid var(--line);
    color: var(--muted);
    font-size: 0.875rem;
    margin-top: 48px;
  }
  .site-footer p {
    margin: 4px 0;
  }
  #main:focus {
    outline: none;
  }
</style>
