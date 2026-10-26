<script lang="ts">
  import '../app.css'
  import { page } from '$app/state'
  import { REPO_URL } from '$lib/site'
  import Horizon from '$lib/components/site/Horizon.svelte'

  let { children } = $props()
  const nav = [
    { href: '/', label: 'Nagi について' },
    { href: '/support', label: 'サポート' },
    { href: '/custom-keyboard', label: 'カスタムキーボード' },
    { href: '/reference', label: 'リファレンス' },
    { href: '/privacy', label: 'プライバシー' }
  ]
  const current = (href: string) => page.url.pathname.replace(/\/$/, '') === href.replace(/\/$/, '')
  // 編集ページは画面いっぱいを使うので、フッターの装飾を省く
  const isTool = $derived(page.url.pathname.startsWith('/custom-keyboard'))
</script>

<a class="skip-link" href="#main">本文へ移動</a>

<header class="site-header">
  <div class="inner" class:wide={isTool}>
    <a class="brand" href="/" aria-label="Nagi ホーム">
      <picture>
        <source srcset="/icon-dark.svg" media="(prefers-color-scheme: dark)" />
        <img src="/icon.svg" alt="" width="30" height="30" />
      </picture>
      <span class="name">Nagi</span>
      <span class="kanji" aria-hidden="true">凪</span>
    </a>
    <nav aria-label="メインメニュー">
      <ul class="nav">
        {#each nav as item (item.href)}
          <li>
            <a href={item.href} aria-current={current(item.href) ? 'page' : undefined}>{item.label}</a>
          </li>
        {/each}
      </ul>
    </nav>
  </div>
</header>

<div id="main" tabindex="-1">
  {@render children()}
</div>

<footer class="site-footer" class:tool={isTool}>
  {#if !isTool}
    <Horizon sun={0.18} />
  {/if}
  <div class="inner">
    <div class="sign">
      <picture>
        <source srcset="/icon-dark.svg" media="(prefers-color-scheme: dark)" />
        <img src="/icon.svg" alt="" width="44" height="44" />
      </picture>
      <div>
        <p class="word"><span>なぎ</span>【凪】</p>
        <p class="def">風がやみ、波が静まること。</p>
      </div>
    </div>
    <p class="about">
      Nagi は個人で開発しているオープンソースの日本語入力システムです。<br />
      コードのライセンスは MIT OR Apache-2.0 です。
    </p>
    <ul class="links">
      <li><a href="/support">サポート</a></li>
      <li><a href="/privacy">プライバシーポリシー</a></li>
      <li><a href="/custom-keyboard">カスタムキーボード</a></li>
      <li><a href="/reference">リファレンス</a></li>
      <li><a href={REPO_URL} rel="noopener">GitHub</a></li>
    </ul>
  </div>
</footer>

<style>
  .site-header {
    position: sticky;
    top: 0;
    z-index: 10;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
  }
  .inner {
    max-width: 1080px;
    margin: 0 auto;
    padding: 0 20px;
    box-sizing: border-box;
  }
  .inner.wide {
    max-width: none;
  }
  .site-header .inner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px 24px;
    min-height: 60px;
    flex-wrap: wrap;
  }
  .brand {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--fg);
    text-decoration: none;
    padding: 6px 0;
  }
  .brand img {
    display: block;
    border-radius: 7px;
  }
  .brand .name {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.1875rem;
    letter-spacing: 0.04em;
  }
  .brand .kanji {
    font-family: var(--font-serif);
    color: var(--muted);
    font-size: 0.875rem;
    padding-left: 10px;
    border-left: 1px solid var(--line);
  }
  .nav {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    gap: 2px 22px;
    flex-wrap: wrap;
  }
  .nav a {
    position: relative;
    display: inline-block;
    padding: 10px 0;
    color: var(--muted);
    text-decoration: none;
    font-size: 0.875rem;
  }
  .nav a:hover {
    color: var(--fg);
  }
  .nav a[aria-current='page'] {
    color: var(--fg);
  }
  /* 現在のページは、変換中の文節のような太い下線で示す */
  .nav a[aria-current='page']::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 4px;
    height: 2px;
    background: var(--fg);
  }
  /* 狭い画面ではメニューを 1 行に収め、はみ出す分は横スクロールにする */
  @media (max-width: 640px) {
    .site-header .inner {
      padding-top: 4px;
      min-height: 0;
    }
    nav {
      width: calc(100% + 40px);
      margin: 0 -20px;
      overflow-x: auto;
      scrollbar-width: none;
    }
    .nav {
      flex-wrap: nowrap;
      gap: 0 18px;
      padding: 0 20px;
      white-space: nowrap;
    }
    .nav a {
      padding: 8px 0 10px;
    }
  }

  .site-footer {
    margin-top: 96px;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .site-footer.tool {
    margin-top: 0;
    border-top: 1px solid var(--line);
  }
  .site-footer .inner {
    display: grid;
    gap: 20px 48px;
    grid-template-columns: minmax(0, 1fr);
    padding-top: 28px;
    padding-bottom: 40px;
  }
  @media (min-width: 760px) {
    .site-footer .inner {
      grid-template-columns: auto minmax(0, 1fr) auto;
      align-items: start;
    }
  }
  .sign {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .sign img {
    display: block;
    border-radius: 10px;
  }
  .sign p {
    margin: 0;
    line-height: 1.5;
  }
  .word {
    font-family: var(--font-serif);
    color: var(--fg);
    font-size: 1rem;
  }
  .word span {
    font-weight: 600;
  }
  .def {
    font-family: var(--font-serif);
  }
  .about {
    margin: 0;
  }
  .links {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }
  .links a {
    color: var(--fg);
    text-decoration: none;
  }
  .links a:hover {
    text-decoration: underline;
  }
  #main:focus {
    outline: none;
  }
</style>
