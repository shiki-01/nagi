<script lang="ts">
  // キャンバスに置くキーボード本体。描画だけを担当し、ポインタ操作は親（KeyboardCanvas）が data 属性
  // （data-row / data-key-id）を手掛かりに処理する。キーはフォーカス可能なボタンで、Enter / Space で選択できる。
  // 見た目（look / states / rules / metrics / background / assets）は resolveLook の結果を CSS に近似して描く
  // （look-css.ts。実機の SwiftUI とは細部が異なる）。指定が無ければ従来の見た目。
  import { describeKey, mainLabel } from '$lib/keyboard/key-display'
  import { backgroundCss, faceCss, metricsCss, PALETTES, paletteVars, resolveBackground, spaceDragOrigin, statesForKey, toStyle } from '$lib/keyboard/look-css'
  import { measureImage } from '$lib/keyboard/look-image'
  import { lookContent, resolveLook } from '$lib/keyboard/look-resolve'
  import { ICONIFY_ICON_PATTERN } from '$lib/keyboard/types'
  import type { Key, KeyboardLayout, StateName } from '$lib/keyboard/types'

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
    /** プレビューで有効にする状態。局所の状態は選択中のキー（無ければ全キー）、大域の状態は全キーに適用する。 */
    previewStates?: readonly StateName[]
    /** プレビューの外観。auto は OS の設定に従う。 */
    scheme?: 'auto' | 'light' | 'dark'
  }
  let { layout, selectedIds = [], dragIds = [], focusId = null, el = $bindable(null), onactivate, previewStates = [], scheme = 'auto' }: Props = $props()

  const selected = $derived(new Set(selectedIds))
  const dragging = $derived(new Set(dragIds))
  const tabId = $derived.by(() => {
    const ids = layout.rows.flatMap((r) => r.keys.map((k) => k.id))
    return focusId && ids.includes(focusId) ? focusId : (ids[0] ?? null)
  })

  // OS の配色・視差効果の設定（ブラウザのみ）
  let osDark = $state(false)
  let reduceMotion = $state(false)
  $effect(() => {
    const dark = window.matchMedia('(prefers-color-scheme: dark)')
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)')
    osDark = dark.matches
    reduceMotion = motion.matches
    const d = (e: MediaQueryListEvent) => (osDark = e.matches)
    const m = (e: MediaQueryListEvent) => (reduceMotion = e.matches)
    dark.addEventListener('change', d)
    motion.addEventListener('change', m)
    return () => {
      dark.removeEventListener('change', d)
      motion.removeEventListener('change', m)
    }
  })
  const eff = $derived(scheme === 'auto' ? (osDark ? 'dark' : 'light') : scheme)
  const palette = $derived(PALETTES[eff])

  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
  const assets = $derived(isObj(layout.assets) ? (layout.assets as Record<string, unknown>) : {})
  const assetUri = (id: unknown): string | undefined => (typeof id === 'string' && typeof assets[id] === 'string' ? (assets[id] as string) : undefined)

  // tile の大きさの計算に、画像の元の大きさ（px）が要る
  let sizes = $state<Record<string, { w: number; h: number }>>({})
  $effect(() => {
    for (const [id, uri] of Object.entries(assets)) {
      if (typeof uri !== 'string') continue
      void measureImage(uri).then((s) => {
        if (s && (sizes[id]?.w !== s.w || sizes[id]?.h !== s.h)) sizes = { ...sizes, [id]: s }
      })
    }
  })

  const origin = $derived(spaceDragOrigin(layout))
  const metrics = $derived(metricsCss(layout))
  const board = $derived.by(() => {
    const bg = resolveBackground(layout, eff)
    const uri = assetUri(bg.fillImage)
    const css = backgroundCss(bg, { palette, image: uri ? { uri, size: sizes[bg.fillImage as string] } : undefined })
    return css ? { face: toStyle(css.face), image: css.image ? toStyle(css.image) : null } : null
  })

  function keyFace(ri: number, key: Key) {
    const look = resolveLook(layout, ri, key, { states: statesForKey(key, selectedIds, previewStates), origin, scheme: eff })
    const uri = assetUri(look.fillImage)
    const css = faceCss(look, key.role ?? 'normal', { palette, reduceMotion, image: uri ? { uri, size: sizes[look.fillImage as string] } : undefined })
    const content = lookContent(look)
    const m = content.icon ? ICONIFY_ICON_PATTERN.exec(content.icon) : null
    const label = content.label ?? mainLabel(key)
    return {
      face: toStyle(css.face),
      image: css.image ? toStyle(css.image) : null,
      label,
      iconUrl: m ? `https://api.iconify.design/${m[1]}/${m[2]}.svg` : null,
      small: look.fontSize === undefined && [...label].length > 3
    }
  }

  // 状態の icon（iconify:）の取得に失敗した URL。失敗したら label を出す
  let iconFailed = $state<Record<string, true>>({})
</script>

<div
  class="phone"
  bind:this={el}
  style:width="{BOARD_WIDTH}px"
  style={toStyle(paletteVars(palette))}
  role="group"
  aria-label="キーボード本体。矢印キーでキーを選択、Delete で削除、Alt と矢印キーで移動"
>
  {#if board}
    <div class="bg" aria-hidden="true" style={board.face}>
      {#if board.image}<span class="img" style={board.image}></span>{/if}
    </div>
  {/if}
  <div class="candbar" aria-hidden="true">
    <span>候補バー</span><span class="dim">きょう　今日　京</span>
  </div>
  <div class="kb" style:gap={metrics.rowSpacing} style:padding={metrics.padding}>
    {#each layout.rows as row, ri (ri)}
      <div class="row" data-row={ri} style:height="{52 * (row.height ?? 1)}px" style:gap={metrics.keySpacing}>
        {#each row.keys as key, ki (ki)}
          {@const f = key.kind === 'spacer' ? null : keyFace(ri, key)}
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
            {#if f}
              <!-- 顔は見た目だけ（拡縮・ずらし・inset をかけても、ボタン＝当たり判定は動かない） -->
              <span class="face {key.role ?? 'normal'}" style={f.face} aria-hidden="true">
                {#if f.image}<span class="img" style={f.image}></span>{/if}
                {#if key.kind === 'flick'}
                  <span class="fl fl-u">{key.up?.text ?? ''}</span>
                  <span class="fl fl-d">{key.down?.text ?? ''}</span>
                  <span class="fl fl-l">{key.left?.text ?? ''}</span>
                  <span class="fl fl-r">{key.right?.text ?? ''}</span>
                {/if}
                {#if f.iconUrl && !iconFailed[f.iconUrl]}
                  <img class="ico" src={f.iconUrl} alt="" height="20" onerror={() => (iconFailed = { ...iconFailed, [f.iconUrl as string]: true })} />
                {:else}
                  <span class="main" class:small={f.small}>{f.label}</span>
                {/if}
                {#if key.longPress?.length}<span class="lp"></span>{/if}
              </span>
            {/if}
          </button>
        {/each}
        {#if row.keys.length === 0}<span class="empty" aria-hidden="true">空の行</span>{/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .phone {
    position: relative;
    border-radius: 18px;
    border: 1px solid var(--line);
    background: var(--kb-bg);
    color: var(--fg);
    box-shadow: var(--panel-shadow);
    user-select: none;
    -webkit-user-select: none;
    touch-action: none;
  }
  /* キーボード全体の背景（layout.background）。最背面。 */
  .bg {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    overflow: hidden;
    pointer-events: none;
  }
  .img {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .candbar {
    position: relative;
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
    position: relative;
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
  /* ボタンは当たり判定と選択の枠だけ。見た目は .face に描く。 */
  .key {
    position: relative;
    flex-basis: 0;
    min-width: 0;
    padding: 0;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--fg);
    cursor: pointer;
    display: block;
    font: inherit;
    font-size: 1.125rem;
    touch-action: none;
  }
  .face {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    box-sizing: border-box;
    pointer-events: none;
  }
  .key.spacer {
    background: transparent;
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
  }
  .key:focus-visible {
    outline: calc(3px / var(--zoom, 1)) solid var(--focus);
    outline-offset: calc(2px / var(--zoom, 1));
    z-index: 2;
  }
  .main {
    position: relative;
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
  }
  .main.small {
    font-size: 0.8125rem;
  }
  .ico {
    position: relative;
    max-width: 100%;
  }
  .fl {
    position: absolute;
    font-size: 0.625rem;
    color: var(--muted);
    line-height: 1;
  }
  .face.accent .fl {
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
