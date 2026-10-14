<script lang="ts">
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { rowWidth } from '$lib/keyboard/ops'
  import { KEY_KINDS, LIMITS } from '$lib/keyboard/types'
  import type { Key, KeyKind } from '$lib/keyboard/types'

  let { store }: { store: EditorStore } = $props()

  let newKind = $state<KeyKind>('text')
  let dragId = $state<string | null>(null)
  let overKey = $state<string | null>(null)
  let overRow = $state<number | null>(null)

  const KIND_SHORT: Record<KeyKind, string> = {
    text: '文字',
    flick: 'フリック',
    toggle: 'トグル',
    action: '操作',
    shortcut: 'ショートカット',
    layout: '切替',
    spacer: '空白'
  }

  function chipLabel(k: Key): string {
    if (k.kind === 'spacer') return '空白'
    return k.label || (k.kind === 'text' ? k.text : k.kind === 'flick' ? k.center.text : k.id) || k.id
  }

  function drop(rowIndex: number, index: number) {
    if (dragId === null) return
    const from = store.layout.rows.flatMap((r, ri) => r.keys.map((k, ki) => ({ id: k.id, ri, ki }))).find((x) => x.id === dragId)
    if (from) {
      const at = from.ri === rowIndex && from.ki < index ? index - 1 : index
      store.moveKey(dragId, rowIndex, at)
    }
    dragId = overKey = overRow = null
  }
  const sum = (n: number) => Math.round(n * 1000) / 1000
</script>

<section aria-labelledby="struct-title">
  <h3 id="struct-title">行とキーの配置</h3>
  <p class="hint">キーを選ぶと右（または下）で設定できます。ドラッグ、または「← →」ボタンで並べ替えます。</p>
  <ol class="rows">
    {#each store.layout.rows as row, ri (ri)}
      {@const w = sum(rowWidth(row))}
      <li class="rowbox" class:over={overRow === ri && overKey === null} aria-label="{ri + 1} 行目">
        <div class="rowhead">
          <strong>{ri + 1} 行目</strong>
          <span class="width" class:bad={w < LIMITS.rowWidthMin || w > LIMITS.rowWidthMax}>幅の合計 {w}（1〜20）</span>
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
            <button type="button" class="btn small danger" aria-label="{ri + 1} 行目を削除" disabled={store.layout.rows.length <= 1} onclick={() => store.removeRow(ri)}>行を削除</button>
          </span>
        </div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="chips"
          role="group"
          aria-label="{ri + 1} 行目のキー"
          ondragover={(e) => {
            if (dragId !== null) {
              e.preventDefault()
              overRow = ri
              overKey = null
            }
          }}
          ondrop={(e) => {
            e.preventDefault()
            drop(ri, row.keys.length)
          }}
        >
          {#each row.keys as k, ki (ki)}
            <button
              type="button"
              class="chip"
              class:sel={store.selectedKeyId === k.id}
              class:drop={overKey === `${ri}:${ki}`}
              class:dragging={dragId === k.id}
              style:flex-grow={Math.min(k.width ?? 1, 3)}
              draggable="true"
              aria-pressed={store.selectedKeyId === k.id}
              aria-label="{chipLabel(k)}（{KIND_SHORT[k.kind]}、幅 {k.width ?? 1}）"
              onclick={() => (store.selectedKeyId = k.id)}
              ondragstart={(e) => {
                dragId = k.id
                e.dataTransfer?.setData('text/plain', k.id)
                if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
              }}
              ondragend={() => (dragId = overKey = overRow = null)}
              ondragover={(e) => {
                if (dragId !== null) {
                  e.preventDefault()
                  e.stopPropagation()
                  overKey = `${ri}:${ki}`
                }
              }}
              ondrop={(e) => {
                e.preventDefault()
                e.stopPropagation()
                drop(ri, ki)
              }}
            >
              <span class="t">{chipLabel(k)}</span>
              <span class="kd">{KIND_SHORT[k.kind]}</span>
            </button>
          {/each}
        </div>
        <div class="add">
          <button type="button" class="btn small" disabled={row.keys.length >= LIMITS.keysPerRowMax} onclick={() => store.addKey(ri, newKind)}>
            ＋ キーを追加（{KIND_SHORT[newKind]}）
          </button>
        </div>
      </li>
    {/each}
  </ol>
  <div class="foot">
    <button type="button" class="btn small" disabled={store.layout.rows.length >= LIMITS.rowsMax} onclick={() => store.addRow()}>＋ 行を追加</button>
    <label class="kindsel">
      追加するキーの種類
      <select bind:value={newKind}>
        {#each KEY_KINDS as k (k)}<option value={k}>{k}（{KIND_SHORT[k]}）</option>{/each}
      </select>
    </label>
  </div>
</section>

<style>
  h3 {
    margin: 0 0 4px;
    font-size: 1.0625rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.875rem;
    margin: 0 0 8px;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }
  .rowbox {
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 8px;
    background: var(--surface);
  }
  .rowbox.over {
    border-color: var(--focus);
  }
  .rowhead {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    align-items: center;
    margin-bottom: 6px;
    font-size: 0.875rem;
  }
  .width {
    color: var(--muted);
  }
  .width.bad {
    color: var(--danger);
    font-weight: 600;
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
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    min-height: 48px;
  }
  .chip {
    flex-basis: 56px;
    min-width: 56px;
    min-height: 48px;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--fg);
    padding: 2px 6px;
    display: grid;
    align-content: center;
    justify-items: center;
    cursor: grab;
    font-size: 0.9375rem;
  }
  .chip.sel {
    border: 2px solid var(--focus);
    background: var(--accent-soft);
  }
  .chip.drop {
    box-shadow: -4px 0 0 var(--focus);
  }
  .chip.dragging {
    opacity: 0.4;
  }
  .t {
    max-width: 8em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kd {
    font-size: 0.6875rem;
    color: var(--muted);
  }
  .add {
    margin-top: 6px;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    margin-top: 10px;
    font-size: 0.875rem;
  }
  .kindsel {
    display: flex;
    gap: 6px;
    align-items: center;
  }
</style>
