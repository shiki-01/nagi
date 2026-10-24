<script lang="ts">
  // キャンバスに置くキーボード本体。描画だけを担当し、ポインタ操作は親（KeyboardCanvas）が data 属性
  // （data-row / data-key-id）を手掛かりに処理する。キーはフォーカス可能なボタンで、Enter / Space で選択できる。
  import { describeKey, mainLabel } from '$lib/keyboard/key-display'
  import type { KeyboardLayout } from '$lib/keyboard/types'

  const BOARD_WIDTH = 430

  interface Props {
    layout: KeyboardLayout
    selectedIds?: readonly string[]
    /** ドラッグ中のキー（薄く表示する）。 */
    dragIds?: readonly string[]
    /** Tab で入れるキー（roving tabindex）。無ければ先頭。 */
    focusId?: string | null
    el?: HTMLElement | null
    onactivate?: (id: string) => void
  }
  let { layout, selectedIds = [], dragIds = [], focusId = null, el = $bindable(null), onactivate }: Props = $props()

  const selected = $derived(new Set(selectedIds))
  const dragging = $derived(new Set(dragIds))
  const tabId = $derived.by(() => {
    const ids = layout.rows.flatMap((r) => r.keys.map((k) => k.id))
    return focusId && ids.includes(focusId) ? focusId : (ids[0] ?? null)
  })
</script>

<div class="phone" bind:this={el} style:width="{BOARD_WIDTH}px" role="group" aria-label="キーボード本体。矢印キーでキーを選択、Delete で削除、Alt と矢印キーで移動">
  <div class="candbar" aria-hidden="true">
    <span>候補バー</span><span class="dim">きょう　今日　京</span>
  </div>
  <div class="kb">
    {#each layout.rows as row, ri (ri)}
      <div class="row" data-row={ri} style:height="{52 * (row.height ?? 1)}px">
        {#each row.keys as key, ki (ki)}
          <button
            type="button"
            data-key-id={key.id}
            class="key {key.kind === 'spacer' ? 'spacer' : (key.role ?? 'normal')}"
            class:selected={selected.has(key.id)}
            class:dragging={dragging.has(key.id)}
            style:flex-grow={key.width ?? 1}
            tabindex={key.id === tabId ? 0 : -1}
            aria-label={describeKey(key)}
            aria-pressed={selected.has(key.id)}
            onclick={(e) => {
              // マウス / タッチはキャンバス側の pointer イベントで処理済み。キーボード操作（detail = 0）だけここで扱う。
              if (e.detail === 0) onactivate?.(key.id)
            }}
          >
            {#if key.kind === 'flick'}
              <span class="fl fl-u" aria-hidden="true">{key.up?.text ?? ''}</span>
              <span class="fl fl-d" aria-hidden="true">{key.down?.text ?? ''}</span>
              <span class="fl fl-l" aria-hidden="true">{key.left?.text ?? ''}</span>
              <span class="fl fl-r" aria-hidden="true">{key.right?.text ?? ''}</span>
              <span class="main" aria-hidden="true">{mainLabel(key)}</span>
            {:else if key.kind !== 'spacer'}
              <span class="main" class:small={mainLabel(key).length > 3} aria-hidden="true">{mainLabel(key)}</span>
            {/if}
            {#if key.longPress?.length}<span class="lp" aria-hidden="true"></span>{/if}
          </button>
        {/each}
        {#if row.keys.length === 0}<span class="empty" aria-hidden="true">空の行</span>{/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .phone {
    border-radius: 18px;
    border: 1px solid var(--line);
    background: var(--kb-bg);
    box-shadow: var(--panel-shadow);
    user-select: none;
    -webkit-user-select: none;
    touch-action: none;
  }
  .candbar {
    display: flex;
    gap: 12px;
    align-items: center;
    height: 40px;
    padding: 0 12px;
    font-size: 0.8125rem;
    color: var(--muted);
    border-bottom: 1px solid var(--line);
  }
  .dim {
    color: var(--fg);
  }
  .kb {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 4px 10px;
  }
  .row {
    display: flex;
    gap: 6px;
    min-height: 14px;
  }
  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.8125rem;
    color: var(--muted);
    border: 1px dashed var(--muted);
    border-radius: 7px;
  }
  .key {
    position: relative;
    flex-basis: 0;
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: var(--key-bg);
    color: var(--fg);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.3);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    font: inherit;
    font-size: 1.125rem;
    overflow: hidden;
    touch-action: none;
  }
  .key.function {
    background: var(--key-fn-bg);
  }
  .key.accent {
    background: var(--key-accent-bg);
    color: var(--key-accent-fg);
  }
  .key.spacer {
    background: transparent;
    box-shadow: none;
    outline: 1px dashed var(--muted);
    outline-offset: -1px;
  }
  .key.dragging {
    opacity: 0.35;
  }
  /* 枠の太さは画面上で一定にする（--zoom は KeyboardCanvas が設定する）。 */
  .key.selected {
    outline: calc(2.5px / var(--zoom, 1)) solid var(--focus);
    outline-offset: calc(1px / var(--zoom, 1));
    z-index: 1;
    overflow: visible;
  }
  .key:focus-visible {
    outline: calc(3px / var(--zoom, 1)) solid var(--focus);
    outline-offset: calc(2px / var(--zoom, 1));
    z-index: 2;
  }
  .main {
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
  }
  .main.small {
    font-size: 0.8125rem;
  }
  .fl {
    position: absolute;
    font-size: 0.625rem;
    color: var(--muted);
    line-height: 1;
  }
  .key.accent .fl {
    color: inherit;
    opacity: 0.8;
  }
  .fl-u {
    top: 3px;
    left: 50%;
    transform: translateX(-50%);
  }
  .fl-d {
    bottom: 3px;
    left: 50%;
    transform: translateX(-50%);
  }
  .fl-l {
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
  }
  .fl-r {
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
  }
  .lp {
    position: absolute;
    top: 3px;
    right: 3px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.55;
  }
</style>
