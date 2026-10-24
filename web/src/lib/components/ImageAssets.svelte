<script lang="ts">
  // 背景画像（layout.assets）の追加・削除。ファイルは canvas で長辺 1024px 以内に縮小し、PNG / JPEG の data URI にして JSON に埋め込む。
  // 個数（4 枚）・1 枚（256KB）・合計（768KB）の上限を超えるものは追加できない。
  import { assetByteBudget, assetIdFromName, assetsUsage, dataUriBytes, formatKB } from '$lib/keyboard/look-assets'
  import { encodeImageFile } from '$lib/keyboard/look-image'
  import { assetRefCount } from '$lib/keyboard/look-ops'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { ASSET_LIMITS } from '$lib/keyboard/types'

  let { store }: { store: EditorStore } = $props()

  const assets = $derived((store.layout.assets && typeof store.layout.assets === 'object' ? store.layout.assets : {}) as Record<string, unknown>)
  const ids = $derived(Object.keys(assets))
  const used = $derived(assetsUsage(assets))
  const full = $derived(ids.length >= ASSET_LIMITS.countMax)
  let busy = $state(false)
  let message = $state('')
  let failure = $state('')

  async function onfile(e: Event & { currentTarget: HTMLInputElement }) {
    const input = e.currentTarget
    const file = input.files?.[0]
    input.value = ''
    if (!file) return
    failure = ''
    message = ''
    busy = true
    try {
      const r = await encodeImageFile(file, assetByteBudget(assets))
      if (!r.ok) {
        failure = r.error
        return
      }
      const id = assetIdFromName(file.name, ids)
      const err = store.addAsset(id, r.dataUri)
      if (err) failure = err
      else message = `画像「${id}」を追加しました（${r.width}×${r.height}px、${r.format === 'image/png' ? 'PNG' : 'JPEG'}、${formatKB(r.bytes)}）。`
    } finally {
      busy = false
    }
  }

  function remove(id: string) {
    const refs = assetRefCount(store.layout, id)
    if (refs > 0 && !confirm(`画像「${id}」は ${refs} か所で使われています。削除すると、それらの背景画像の指定も外れます。よろしいですか？（元に戻すで復元できます）`)) return
    store.removeAsset(id)
    message = `画像「${id}」を削除しました。`
    failure = ''
  }
</script>

<section class="assets" aria-labelledby="assets-h">
  <h4 id="assets-h">画像</h4>
  <p class="usage">
    <meter min="0" max={ASSET_LIMITS.totalMax} low={ASSET_LIMITS.totalMax * 0.75} high={ASSET_LIMITS.totalMax * 0.9} optimum="0" value={used} aria-label="画像の合計の使用量"></meter>
    <span>使用量 {formatKB(used)} / {formatKB(ASSET_LIMITS.totalMax)}・{ids.length} / {ASSET_LIMITS.countMax} 枚</span>
  </p>
  {#if ids.length}
    <ul class="list">
      {#each ids as id (id)}
        {@const bytes = dataUriBytes(assets[id])}
        <li>
          {#if bytes !== undefined}<img src={assets[id] as string} alt="" width="40" height="40" />{:else}<span class="bad" aria-hidden="true">?</span>{/if}
          <span class="meta"
            ><strong>{id}</strong>
            {#if bytes !== undefined}<span>{formatKB(bytes)}・{assetRefCount(store.layout, id)} か所で使用</span>{:else}<span class="err">読み込めない画像です（検証エラー）</span>{/if}</span
          >
          <button type="button" class="btn small danger" aria-label="画像 {id} を削除" onclick={() => remove(id)}>削除</button>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="hint">画像はありません。</p>
  {/if}
  <label class="file" class:disabled={full || busy}>
    <span>{busy ? '変換中…' : '画像を追加（PNG / JPEG）'}</span>
    <input type="file" accept="image/png,image/jpeg" disabled={full || busy} onchange={onfile} />
  </label>
  <p class="hint">
    追加した画像は長辺 {ASSET_LIMITS.longSideMax}px 以内に縮小され、透過が無ければ JPEG（品質を調整）、あれば PNG で JSON に埋め込まれます。1 枚 {ASSET_LIMITS.bytesMax / 1024}KB・合計
    {ASSET_LIMITS.totalMax / 1024}KB・{ASSET_LIMITS.countMax} 枚までです。画像を含むレイアウトは URL では渡せません（ファイルかコピーで渡してください）。
  </p>
  {#if full}<p class="err" role="status">画像は {ASSET_LIMITS.countMax} 枚までです。追加するには削除してください。</p>{/if}
  {#if failure}<p class="err" role="alert">{failure}</p>{/if}
  <p class="ok" role="status" aria-live="polite">{message}</p>
</section>

<style>
  .assets {
    display: grid;
    gap: 8px;
  }
  h4 {
    margin: 0;
    font-size: 0.9375rem;
  }
  .usage {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 0.8125rem;
  }
  meter {
    width: 120px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    border: 1px solid var(--line);
    border-radius: 10px;
  }
  li img,
  .bad {
    width: 40px;
    height: 40px;
    object-fit: cover;
    border-radius: 6px;
    background: var(--surface-2);
    display: grid;
    place-items: center;
  }
  .meta {
    flex: 1;
    display: grid;
    min-width: 0;
    font-size: 0.8125rem;
  }
  .meta span {
    color: var(--muted);
  }
  .file {
    display: grid;
    gap: 4px;
    font-size: 0.875rem;
  }
  .file.disabled {
    opacity: 0.55;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .err {
    margin: 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  .meta .err {
    color: var(--danger);
  }
  .ok {
    min-height: 1.2em;
    margin: 0;
    color: var(--ok);
    font-size: 0.8125rem;
  }
</style>
