<script lang="ts">
  // キー未選択時のインスペクタ。レイアウト全体の情報（name / id / mode）と、各行の高さ・並びを編集する。
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { rowWidth } from '$lib/keyboard/ops'
  import { LAYOUT_MODES, LIMITS } from '$lib/keyboard/types'
  import type { LayoutMode } from '$lib/keyboard/types'

  let { store }: { store: EditorStore } = $props()

  const MODE_LABELS: Record<LayoutMode, string> = {
    kana: 'kana（かな）',
    alpha: 'alpha（英字）',
    number: 'number（数字）',
    symbol: 'symbol（記号）',
    emoji: 'emoji（絵文字）',
    custom: 'custom'
  }
  const sum = (n: number) => Math.round(n * 1000) / 1000
  const heightSum = $derived(sum(store.layout.rows.reduce((a, r) => a + (r.height ?? 1), 0)))
</script>

<section class="meta" aria-labelledby="meta-h">
  <h3 id="meta-h">レイアウトの情報</h3>
  <div class="grid">
    <label class="field">
      <span>名前 (name)</span>
      <input value={store.layout.name} maxlength={LIMITS.nameMax} oninput={(e) => store.setMeta({ name: e.currentTarget.value }, 'name')} />
    </label>
    <label class="field">
      <span>id（ファイル名にも使用）</span>
      <input value={store.layout.id} spellcheck="false" oninput={(e) => store.setMeta({ id: e.currentTarget.value }, 'id')} />
    </label>
    <label class="field">
      <span>モード (mode)</span>
      <select value={store.layout.mode} onchange={(e) => store.setMeta({ mode: e.currentTarget.value as LayoutMode })}>
        {#each LAYOUT_MODES as m (m)}<option value={m}>{MODE_LABELS[m]}</option>{/each}
      </select>
    </label>
  </div>

  <h3 id="rows-h">行</h3>
  <p class="hint" class:bad={heightSum > LIMITS.rowHeightSumMax}>高さの比の合計 {heightSum}（上限 {LIMITS.rowHeightSumMax}）</p>
  <ol class="rows" aria-labelledby="rows-h">
    {#each store.layout.rows as row, ri (ri)}
      {@const w = sum(rowWidth(row))}
      <li class="row">
        <strong>{ri + 1} 行目</strong>
        <span class="w" class:bad={w < LIMITS.rowWidthMin || w > LIMITS.rowWidthMax}>幅の合計 {w}（{LIMITS.rowWidthMin}〜{LIMITS.rowWidthMax}）</span>
        <label class="h">
          高さ
          <input
            type="number"
            min="0.25"
            max={LIMITS.rowHeightMax}
            step="0.25"
            value={row.height ?? 1}
            aria-label="{ri + 1} 行目の高さの比"
            oninput={(e) => {
              const n = Number(e.currentTarget.value)
              store.setRowHeight(ri, e.currentTarget.value === '' || !Number.isFinite(n) ? undefined : n)
            }}
          />
        </label>
        <span class="ctl">
          <button type="button" class="btn small" aria-label="{ri + 1} 行目を上へ" disabled={ri === 0} onclick={() => store.moveRow(ri, ri - 1)}>↑</button>
          <button type="button" class="btn small" aria-label="{ri + 1} 行目を下へ" disabled={ri === store.layout.rows.length - 1} onclick={() => store.moveRow(ri, ri + 1)}>↓</button>
          <button type="button" class="btn small danger" aria-label="{ri + 1} 行目を削除" disabled={store.layout.rows.length <= 1} onclick={() => store.removeRow(ri)}>削除</button>
        </span>
      </li>
    {/each}
  </ol>
  <button type="button" class="btn small" disabled={store.layout.rows.length >= LIMITS.rowsMax} onclick={() => store.addRow()}>＋ 行を追加</button>
  <p class="hint">キーはキャンバス上でドラッグして並べ替えます。行と行の間（行の上下の縁）にドロップすると新しい行ができます。</p>

  <h3 id="toolbar-h">ツールバー</h3>
  <p class="hint">
    候補バーが空のときに出る、高さ 44pt の 1 行です（{LIMITS.toolbarKeysMin}〜{LIMITS.toolbarKeysMax} 個）。キーはキャンバス上部の行に並び、行と同じ操作（追加・ドラッグで並べ替え・削除・インスペクタでの編集）ができます。
    無いときは、アプリの設定のツールバー項目が使われます。action に <code>paste</code>（貼り付け）・<code>deleteWord</code>（単語削除）も使えます。
  </p>
  {#if store.layout.toolbar}
    <p class="hint">キー {store.layout.toolbar.keys.length} 個</p>
    <button type="button" class="btn small danger" onclick={() => store.removeToolbar()}>ツールバーを削除</button>
  {:else}
    <button type="button" class="btn small" onclick={() => store.addToolbar()}>＋ ツールバーを追加</button>
  {/if}
</section>

<style>
  .meta {
    display: grid;
    gap: 10px;
  }
  h3 {
    margin: 0;
    font-size: 1rem;
  }
  .grid {
    display: grid;
    gap: 10px;
  }
  .field {
    display: grid;
    gap: 2px;
    font-size: 0.875rem;
  }
  .field span {
    color: var(--muted);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .bad {
    color: var(--danger);
    font-weight: 600;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    align-items: center;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: 10px;
    font-size: 0.875rem;
  }
  .w {
    color: var(--muted);
  }
  .w.bad {
    color: var(--danger);
  }
  .h {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .h input {
    width: 4.5em;
  }
  .ctl {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }
</style>
