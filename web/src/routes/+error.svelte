<script lang="ts">
  import { page } from '$app/state'

  const notFound = $derived(page.status === 404)
</script>

<svelte:head>
  <title>{notFound ? 'ページが見つかりません' : 'エラー'} - Nagi</title>
  <meta name="robots" content="noindex" />
</svelte:head>

<main class="doc">
  <header class="doc-head">
    <p class="eyebrow">{page.status}</p>
    <h1>{notFound ? 'ページが見つかりません' : 'エラーが発生しました'}</h1>
    <p class="lead">
      {notFound ? 'お探しのページは存在しないか、移動した可能性があります。' : '時間をおいてもう一度お試しください。'}
    </p>
  </header>
  <ul class="links">
    <li><a href="/">トップページ</a></li>
    <li><a href="/support">サポート</a></li>
  </ul>
</main>

<style>
  .links {
    list-style: none;
    display: flex;
    gap: 24px;
    padding: 0;
    margin: 24px 0 0;
  }
</style>
