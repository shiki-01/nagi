<script lang="ts">
  import { page } from '$app/state'
</script>

<svelte:head>
  <title>{page.status === 404 ? 'ページが見つかりません' : 'エラー'} - Nagi</title>
  <meta name="robots" content="noindex" />
</svelte:head>

<main class="page">
  <h1>{page.status === 404 ? 'ページが見つかりません' : 'エラーが発生しました'}</h1>
  <p>
    {page.status === 404 ? 'お探しのページは存在しないか、移動した可能性があります。' : '時間をおいてもう一度お試しください。'}
  </p>
  <ul>
    <li><a href="/">トップページ</a></li>
    <li><a href="/support">サポート</a></li>
  </ul>
</main>

<style>
  .page {
    max-width: 760px;
    margin: 0 auto;
    padding: 24px 16px 0;
    box-sizing: border-box;
  }
</style>
