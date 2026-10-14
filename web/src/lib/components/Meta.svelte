<script lang="ts">
  import { SITE_NAME, SITE_URL } from '$lib/site'

  let { title, description, path }: { title: string; description: string; path: string } = $props()
  const full = $derived(title === SITE_NAME ? title : `${title} - ${SITE_NAME}`)
  const url = $derived(SITE_URL + (path === '/' ? '/' : path))
</script>

<svelte:head>
  <title>{full}</title>
  <meta name="description" content={description} />
  <link rel="canonical" href={url} />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content={SITE_NAME} />
  <meta property="og:locale" content="ja_JP" />
  <meta property="og:title" content={full} />
  <meta property="og:description" content={description} />
  <meta property="og:url" content={url} />
  <meta property="og:image" content={`${SITE_URL}/og.svg`} />
  <meta name="twitter:card" content="summary" />
  <meta name="twitter:title" content={full} />
  <meta name="twitter:description" content={description} />
</svelte:head>
