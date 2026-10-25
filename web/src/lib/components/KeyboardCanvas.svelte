<script lang="ts">
  // 無限キャンバス。キーボード本体を CSS transform でパン / ズームし、Pointer Events でキーの選択・範囲選択・
  // ドラッグ＆ドロップ（並べ替え・行の作成・パレットからの追加）を行う。ロジックの大半は純関数
  // （keyboard/ops.ts, keyboard/canvas-geometry.ts）に置き、ここは入出力の配線だけにする。
  import { tick } from 'svelte'
  import KeyboardPreview from './KeyboardPreview.svelte'
  import { clampScale, computeDropTarget, computeToolbarDrop, dropIndicator, fitView, normalizeRect, rectsIntersect, zoomAt } from '$lib/keyboard/canvas-geometry'
  import type { DropTarget, Rect, RowGeom, View } from '$lib/keyboard/canvas-geometry'
  import { KIND_NAMES, mainLabel } from '$lib/keyboard/key-display'
  import { findKey, navigateKey } from '$lib/keyboard/ops'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { KEY_KINDS, TOOLBAR_ROW } from '$lib/keyboard/types'
  import type { KeyKind } from '$lib/keyboard/types'

  interface Props {
    store: EditorStore
    /** false の間（保存済みの状態を読み込む前）はフィットしない。 */
    ready?: boolean
  }
  let { store, ready = true }: Props = $props()

  const DRAG_THRESHOLD = 5

  let view = $state<View>({ x: 0, y: 0, scale: 1 })
  let viewportEl = $state<HTMLElement | null>(null)
  let boardEl = $state<HTMLElement | null>(null)
  let tool = $state<'select' | 'hand'>('select')
  let spaceDown = $state(false)
  let panning = $state(false)
  /** ビューポート内の座標（px）。 */
  let marquee = $state<Rect | null>(null)
  let drag = $state<{ ids: string[]; newKind: KeyKind | null; label: string; x: number; y: number; target: DropTarget | null; indicator: Rect | null } | null>(null)
  let size = { w: 0, h: 0 }
  let needFit = true
  /** ユーザーがパン / ズームしたら、ウィンドウやシートの大きさが変わっても勝手にフィットし直さない。 */
  let userMoved = false

  type Gesture =
    | { t: 'pan'; id: number; x: number; y: number }
    | { t: 'press'; id: number; sx: number; sy: number; keyId: string; additive: boolean; started: boolean }
    | { t: 'palette'; id: number; kind: KeyKind; sx: number; sy: number; started: boolean }
    | { t: 'marquee'; id: number; sx: number; sy: number; base: string[]; started: boolean }
  let g: Gesture | null = null
  const pointers = new Map<number, { x: number; y: number }>()
  let pinch: { dist: number; x: number; y: number } | null = null

  const hand = $derived(tool === 'hand' || spaceDown)
  const gridStep = $derived.by(() => {
    let s = 24 * view.scale
    while (s < 12) s *= 4
    return s
  })
  const focusId = $derived(store.selectedIds.at(-1) ?? null)

  // --- 表示の操作 ---
  export function fit() {
    if (!viewportEl || !boardEl || size.w === 0) {
      needFit = true
      return
    }
    needFit = false
    userMoved = false
    view = fitView(size.w, size.h, boardEl.offsetWidth, boardEl.offsetHeight)
  }
  function zoomBy(factor: number, cx = size.w / 2, cy = size.h / 2) {
    userMoved = true
    view = zoomAt(view, factor, cx, cy)
  }
  function zoomTo(scale: number) {
    zoomBy(clampScale(scale) / view.scale)
  }

  $effect(() => {
    const el = viewportEl
    if (!el) return
    const ro = new ResizeObserver(() => {
      size = { w: el.clientWidth, h: el.clientHeight }
      if ((needFit || !userMoved) && size.w > 0) fit()
    })
    ro.observe(el)
    const onwheel = (e: WheelEvent) => {
      e.preventDefault()
      userMoved = true
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? size.h : 1
      const dx = e.deltaX * unit
      const dy = e.deltaY * unit
      if (e.ctrlKey || e.metaKey) {
        const r = el.getBoundingClientRect()
        // トラックパッドのピンチは ctrlKey 付きの小さな delta が連続で届く。マウスホイールは 1 ノッチが大きい（±100）ので
        // 1 イベントあたりの変化量を抑える。
        const d = Math.max(-80, Math.min(80, dy))
        view = zoomAt(view, Math.exp(-d * 0.004), e.clientX - r.left, e.clientY - r.top)
      } else if (e.shiftKey && dx === 0) {
        view = { ...view, x: view.x - dy }
      } else {
        view = { ...view, x: view.x - dx, y: view.y - dy }
      }
    }
    el.addEventListener('wheel', onwheel, { passive: false })
    return () => {
      ro.disconnect()
      el.removeEventListener('wheel', onwheel)
    }
  })

  $effect(() => {
    if (ready) tick().then(fit)
  })

  // 通知は数秒で消す。
  $effect(() => {
    if (!store.notice) return
    const t = setTimeout(() => (store.notice = null), 4000)
    return () => clearTimeout(t)
  })

  // --- 幾何 ---
  const local = (x: number, y: number) => {
    const r = viewportEl?.getBoundingClientRect()
    return { x: x - (r?.left ?? 0), y: y - (r?.top ?? 0) }
  }
  const toRect = (r: DOMRect): Rect => ({ left: r.left, top: r.top, right: r.right, bottom: r.bottom })
  function collectRows(): RowGeom[] {
    if (!boardEl) return []
    return [...boardEl.querySelectorAll<HTMLElement>('[data-row]')].map((row) => ({
      rect: toRect(row.getBoundingClientRect()),
      keys: [...row.querySelectorAll<HTMLElement>('[data-key-id]')].map((k) => ({ id: k.dataset.keyId ?? '', rect: toRect(k.getBoundingClientRect()) }))
    }))
  }
  /** ツールバー（あれば）の位置とキーの位置。 */
  function collectToolbar(): RowGeom | null {
    const tb = boardEl?.querySelector<HTMLElement>('[data-toolbar]')
    if (!tb) return null
    return { rect: toRect(tb.getBoundingClientRect()), keys: [...tb.querySelectorAll<HTMLElement>('[data-key-id]')].map((k) => ({ id: k.dataset.keyId ?? '', rect: toRect(k.getBoundingClientRect()) })) }
  }
  function keyEl(id: string): HTMLElement | null {
    return boardEl ? ([...boardEl.querySelectorAll<HTMLElement>('[data-key-id]')].find((e) => e.dataset.keyId === id) ?? null) : null
  }
  async function focusKey(id: string | undefined) {
    await tick()
    ;(id ? keyEl(id) : null)?.focus({ preventScroll: true })
    if (!id || !keyEl(id)) viewportEl?.focus({ preventScroll: true })
  }

  function updateDrag(x: number, y: number) {
    if (!drag) return
    const rows = collectRows()
    const exclude = new Set(drag.ids)
    // ツールバーの上ではツールバーへの挿入。それ以外は rows への挿入（新しい行を含む）
    const tb = collectToolbar()
    const tbTarget = tb ? computeToolbarDrop(tb, x, y, exclude) : null
    const target = tbTarget ?? computeDropTarget(rows, x, y, exclude)
    const ind = target ? (tbTarget && tb ? dropIndicator([tb], { kind: 'row', row: 0, index: tbTarget.kind === 'row' ? tbTarget.index : 0 }, exclude) : dropIndicator(rows, target, exclude)) : null
    const vp = viewportEl?.getBoundingClientRect()
    const toLocal = (r: Rect): Rect => ({ left: r.left - (vp?.left ?? 0), right: r.right - (vp?.left ?? 0), top: r.top - (vp?.top ?? 0), bottom: r.bottom - (vp?.top ?? 0) })
    drag = { ...drag, ...local(x, y), target, indicator: ind ? toLocal(ind) : null }
  }

  function startKeyDrag(keyId: string, x: number, y: number) {
    const sel = store.selectedIds
    const ids = sel.includes(keyId) ? sel : [...sel, keyId]
    if (!sel.includes(keyId)) store.selectedIds = ids
    const first = findKey(store.layout, ids[0])?.key
    const label = (first ? mainLabel(first) || first.id : '') + (ids.length > 1 ? ` ほか ${ids.length - 1}` : '')
    drag = { ids, newKind: null, label, x: 0, y: 0, target: null, indicator: null }
    updateDrag(x, y)
  }
  function startPaletteDrag(kind: KeyKind, x: number, y: number) {
    drag = { ids: [], newKind: kind, label: `＋ ${KIND_NAMES[kind]}`, x: 0, y: 0, target: null, indicator: null }
    updateDrag(x, y)
  }
  function dropNow() {
    const d = drag
    drag = null
    if (!d?.target) return
    const dest = d.target.kind === 'row' ? { row: d.target.row, index: d.target.index } : { newRow: d.target.row }
    if (d.newKind) {
      if (store.addKeyAt(d.newKind, dest)) focusKey(store.selectedIds[0])
    } else if (store.moveKeys(d.ids, dest)) {
      focusKey(d.ids[0])
    }
  }

  function selectInRect(m: Rect, base: string[]) {
    if (!boardEl) return
    const vp = viewportEl?.getBoundingClientRect()
    const client: Rect = { left: m.left + (vp?.left ?? 0), right: m.right + (vp?.left ?? 0), top: m.top + (vp?.top ?? 0), bottom: m.bottom + (vp?.top ?? 0) }
    const hit = [...boardEl.querySelectorAll<HTMLElement>('[data-key-id]')].filter((e) => rectsIntersect(toRect(e.getBoundingClientRect()), client)).map((e) => e.dataset.keyId ?? '')
    store.selectedIds = [...base, ...hit]
  }

  // --- ポインタ ---
  function onpointerdown(e: PointerEvent) {
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()]
      g = null
      marquee = null
      drag = null
      panning = false
      const p = local((a.x + b.x) / 2, (a.y + b.y) / 2)
      pinch = { dist: Math.hypot(a.x - b.x, a.y - b.y) || 1, ...p }
      return
    }
    if (pointers.size > 1) return
    const startPan = e.button === 1 || (e.button === 0 && hand)
    if (startPan) {
      e.preventDefault()
      g = { t: 'pan', id: e.pointerId, x: e.clientX, y: e.clientY }
      panning = true
      return
    }
    if (e.button !== 0) return
    const additive = e.shiftKey || e.metaKey || e.ctrlKey
    const target = (e.target as Element).closest<HTMLElement>('[data-key-id]')
    if (target) {
      const id = target.dataset.keyId ?? ''
      if (!additive && !store.selectedIds.includes(id)) store.selectedIds = [id]
      g = { t: 'press', id: e.pointerId, sx: e.clientX, sy: e.clientY, keyId: id, additive, started: false }
      // ブラウザ既定のフォーカス移動に任せるとドラッグ中に選択がずれるので、明示的にフォーカスする。
      e.preventDefault()
      target.focus({ preventScroll: true })
      return
    }
    e.preventDefault()
    viewportEl?.focus({ preventScroll: true })
    if (e.pointerType === 'touch') {
      // タッチでは 1 本指のドラッグでパン（範囲選択はマウス / ペン）。
      g = { t: 'pan', id: e.pointerId, x: e.clientX, y: e.clientY }
      panning = true
      return
    }
    const base = additive ? store.selectedIds : []
    if (!additive) store.selectedIds = []
    g = { t: 'marquee', id: e.pointerId, sx: e.clientX, sy: e.clientY, base, started: false }
  }

  function onpointermove(e: PointerEvent) {
    if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    if (pinch && pointers.size >= 2) {
      userMoved = true
      const [a, b] = [...pointers.values()]
      const dist = Math.hypot(a.x - b.x, a.y - b.y) || 1
      const mid = local((a.x + b.x) / 2, (a.y + b.y) / 2)
      const zoomed = zoomAt(view, dist / pinch.dist, pinch.x, pinch.y)
      view = { ...zoomed, x: zoomed.x + (mid.x - pinch.x), y: zoomed.y + (mid.y - pinch.y) }
      pinch = { dist, ...mid }
      return
    }
    if (!g || g.id !== e.pointerId) return
    switch (g.t) {
      case 'pan':
        userMoved = true
        view = { ...view, x: view.x + e.clientX - g.x, y: view.y + e.clientY - g.y }
        g.x = e.clientX
        g.y = e.clientY
        break
      case 'press':
        if (!g.started && Math.hypot(e.clientX - g.sx, e.clientY - g.sy) > DRAG_THRESHOLD) {
          g.started = true
          startKeyDrag(g.keyId, e.clientX, e.clientY)
        } else if (g.started) updateDrag(e.clientX, e.clientY)
        break
      case 'palette':
        if (!g.started && Math.hypot(e.clientX - g.sx, e.clientY - g.sy) > DRAG_THRESHOLD) {
          g.started = true
          startPaletteDrag(g.kind, e.clientX, e.clientY)
        } else if (g.started) updateDrag(e.clientX, e.clientY)
        break
      case 'marquee': {
        if (!g.started && Math.hypot(e.clientX - g.sx, e.clientY - g.sy) <= DRAG_THRESHOLD) break
        g.started = true
        const a = local(g.sx, g.sy)
        const b = local(e.clientX, e.clientY)
        marquee = normalizeRect(a.x, a.y, b.x, b.y)
        selectInRect(marquee, g.base)
        break
      }
    }
  }

  function endGesture(e: PointerEvent, cancelled: boolean) {
    pointers.delete(e.pointerId)
    if (pointers.size < 2) pinch = null
    if (!g || g.id !== e.pointerId) return
    const cur = g
    g = null
    panning = false
    marquee = null
    if (cancelled) {
      drag = null
      return
    }
    if (cur.t === 'press') {
      if (cur.started) dropNow()
      else if (cur.additive) {
        store.selectedIds = store.selectedIds.includes(cur.keyId) ? store.selectedIds.filter((x) => x !== cur.keyId) : [...store.selectedIds, cur.keyId]
      } else store.selectedIds = [cur.keyId]
    } else if (cur.t === 'palette') {
      if (cur.started) dropNow()
      else quickAdd(cur.kind)
    }
  }

  function paletteDown(e: PointerEvent, kind: KeyKind) {
    if (e.button !== 0) return
    e.preventDefault()
    ;(e.currentTarget as HTMLElement).focus({ preventScroll: true })
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    g = { t: 'palette', id: e.pointerId, kind, sx: e.clientX, sy: e.clientY, started: false }
  }

  /** 選択中のキーの右隣（無ければ最後の行の末尾）にキーを追加する。 */
  function quickAdd(kind: KeyKind) {
    const found = store.selectedIds.length ? findKey(store.layout, store.selectedIds.at(-1) ?? '') : undefined
    const row = found?.row ?? store.layout.rows.length - 1
    const index = found ? found.index + 1 : (store.layout.rows[row]?.keys.length ?? 0)
    if (row < 0) store.addKeyAt(kind, { newRow: 0 })
    else store.addKeyAt(kind, { row, index })
    focusKey(store.selectedIds[0])
  }

  // --- キーボード ---
  const editable = (t: EventTarget | null) => {
    const el = t as HTMLElement | null
    return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)
  }
  let rootEl = $state<HTMLElement | null>(null)
  const inCanvas = (t: EventTarget | null) => t === document.body || (!!rootEl && rootEl.contains(t as Node))

  function onwindowkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && (g || drag)) {
      g = null
      drag = null
      marquee = null
      panning = false
      return
    }
    if (e.key === ' ' && inCanvas(e.target) && !editable(e.target) && !e.ctrlKey && !e.metaKey) {
      e.preventDefault()
      spaceDown = true
    }
  }
  function onwindowkeyup(e: KeyboardEvent) {
    if (e.key === ' ') {
      if (spaceDown) e.preventDefault()
      spaceDown = false
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (editable(e.target)) return
    const mod = e.metaKey || e.ctrlKey
    const arrows: Record<string, 'left' | 'right' | 'up' | 'down'> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' }
    const dir = arrows[e.key]
    if (dir && !mod) {
      e.preventDefault()
      if (e.altKey) {
        store.nudgeSelected(dir)
        focusKey(store.selectedIds.at(-1))
        return
      }
      const from = store.selectedIds.at(-1) ?? ''
      const next = from ? navigateKey(store.layout, from, dir) : store.layout.rows[0]?.keys[0]?.id
      if (!next) return
      store.selectedIds = e.shiftKey ? [...store.selectedIds.filter((x) => x !== next), next] : [next]
      focusKey(next)
      return
    }
    if (mod && e.key.toLowerCase() === 'a') {
      e.preventDefault()
      store.selectAll()
    } else if (mod && e.key.toLowerCase() === 'd') {
      e.preventDefault()
      if (store.selectedKeyId) store.duplicateKey(store.selectedKeyId)
    } else if (!mod && (e.key === 'Delete' || e.key === 'Backspace')) {
      e.preventDefault()
      store.removeSelected()
      tick().then(() => {
        if (document.activeElement === document.body || !document.activeElement) viewportEl?.focus({ preventScroll: true })
      })
    } else if (!mod && e.key === 'Escape') {
      store.selectedIds = []
      viewportEl?.focus({ preventScroll: true })
    } else if (!mod && (e.key === '+' || e.key === '=')) zoomBy(1.2)
    else if (!mod && (e.key === '-' || e.key === '_')) zoomBy(1 / 1.2)
    else if (!mod && e.key.toLowerCase() === 'f') fit()
    else if (!mod && e.key.toLowerCase() === 'h') tool = 'hand'
    else if (!mod && e.key.toLowerCase() === 'v') tool = 'select'
  }

  const dropText = $derived(
    !drag ? '' : !drag.target ? 'ここには置けません' : drag.target.kind === 'row' && drag.target.row === TOOLBAR_ROW ? `ツールバーの ${drag.target.index + 1} 番目に置く` : drag.target.kind === 'newRow' ? `${drag.target.row + 1} 行目として新しい行を作る` : `${drag.target.row + 1} 行目の ${drag.target.index + 1} 番目に置く`
  )
</script>

<svelte:window onpointermove={onpointermove} onpointerup={(e) => endGesture(e, false)} onpointercancel={(e) => endGesture(e, true)} onkeydown={onwindowkeydown} onkeyup={onwindowkeyup} />

<div class="canvas" bind:this={rootEl}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
  <div
    class="viewport"
    class:hand
    class:panning
    class:dragging={!!drag}
    bind:this={viewportEl}
    role="application"
    aria-label="キーボード編集キャンバス。矢印キーでキーを選択、Shift で追加選択、Delete で削除、Alt と矢印キーで移動、プラス・マイナスキーで拡大縮小、F キーで全体表示"
    tabindex="0"
    style:background-size="{gridStep}px {gridStep}px"
    style:background-position="{view.x}px {view.y}px"
    {onpointerdown}
    {onkeydown}
  >
    <div class="world" style:transform="translate({view.x}px, {view.y}px) scale({view.scale})" style:--zoom={view.scale}>
      <KeyboardPreview
        layout={store.layout}
        selectedIds={store.selectedIds}
        dragIds={drag?.ids ?? []}
        {focusId}
        bind:el={boardEl}
        onactivate={(id) => (store.selectedIds = [id])}
        previewStates={store.inspectorTab === 'look' ? store.previewStates : []}
        scheme={store.previewScheme}
      />
    </div>

    {#if marquee}
      <div class="marquee" aria-hidden="true" style:left="{marquee.left}px" style:top="{marquee.top}px" style:width="{marquee.right - marquee.left}px" style:height="{marquee.bottom - marquee.top}px"></div>
    {/if}
    {#if drag?.indicator}
      <div
        class="indicator"
        aria-hidden="true"
        style:left="{drag.indicator.left}px"
        style:top="{drag.indicator.top}px"
        style:width="{drag.indicator.right - drag.indicator.left}px"
        style:height="{drag.indicator.bottom - drag.indicator.top}px"
      ></div>
    {/if}
    {#if drag}
      <div class="ghost" class:invalid={!drag.target} aria-hidden="true" style:left="{drag.x}px" style:top="{drag.y}px">
        <span class="gl">{drag.label}</span>
        <span class="gt">{dropText}</span>
      </div>
    {/if}
  </div>

  <div class="palette" role="toolbar" aria-label="キーの種類。クリックで追加、キャンバスへドラッグで好きな位置に追加">
    <span class="plabel">追加</span>
    {#each KEY_KINDS as kind (kind)}
      <button
        type="button"
        class="pbtn"
        title="{kind}: ドラッグで配置、クリックで選択中のキーの右に追加"
        onpointerdown={(e) => paletteDown(e, kind)}
        onclick={(e) => {
          if (e.detail === 0) quickAdd(kind)
        }}
      >
        <span class="pk">{kind}</span><span class="pn">{KIND_NAMES[kind]}</span>
      </button>
    {/each}
    <span class="psep" aria-hidden="true"></span>
    {#if store.layout.toolbar}
      <button type="button" class="pbtn" title="候補バーが空のときに出るツールバーを削除します（元に戻すで復元できます）" onclick={() => store.removeToolbar()}>
        <span class="pk">ツールバー</span><span class="pn">削除</span>
      </button>
    {:else}
      <button type="button" class="pbtn" title="候補バーが空のときに出るツールバー（高さ 44pt の 1 行）を追加します" onclick={() => store.addToolbar()}>
        <span class="pk">ツールバー</span><span class="pn">追加</span>
      </button>
    {/if}
  </div>

  <div class="zoom" role="toolbar" aria-label="表示の操作">
    <button type="button" class="zbtn" class:on={tool === 'hand'} aria-pressed={tool === 'hand'} title="手のひらツール（H）: ドラッグで移動。Space を押しながらでも移動できます" onclick={() => (tool = tool === 'hand' ? 'select' : 'hand')}>✋</button>
    <button type="button" class="zbtn" aria-label="縮小" onclick={() => zoomBy(1 / 1.2)}>−</button>
    <button type="button" class="zbtn pct" aria-label="表示倍率 {Math.round(view.scale * 100)}%。押すと 100% に戻す" onclick={() => zoomTo(1)}>{Math.round(view.scale * 100)}%</button>
    <button type="button" class="zbtn" aria-label="拡大" onclick={() => zoomBy(1.2)}>＋</button>
    <button type="button" class="zbtn fit" title="キーボード全体を表示（F）" onclick={fit}>フィット</button>
  </div>

  <p class="help" aria-hidden="true">Ctrl/⌘+ホイール: 拡大縮小 ・ Space+ドラッグ: 移動 ・ Shift+クリック: 複数選択</p>
  <p class="toast" role="status" aria-live="polite">{store.notice ?? ''}</p>
</div>

<style>
  .canvas {
    position: relative;
    width: 100%;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .viewport {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background-color: var(--canvas-bg);
    background-image: radial-gradient(circle, var(--canvas-dot) 1px, transparent 1.2px);
    touch-action: none;
    overscroll-behavior: contain;
    user-select: none;
    -webkit-user-select: none;
    cursor: default;
    outline-offset: -3px;
  }
  .viewport.hand {
    cursor: grab;
  }
  .viewport.panning {
    cursor: grabbing;
  }
  .viewport.dragging {
    cursor: copy;
  }
  .world {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    width: max-content;
  }
  .viewport.hand .world {
    pointer-events: none;
  }
  .marquee {
    position: absolute;
    pointer-events: none;
    border: 1px solid var(--focus);
    background: var(--select-fill);
  }
  .indicator {
    position: absolute;
    pointer-events: none;
    background: var(--drop-indicator);
    border-radius: 2px;
    box-shadow: 0 0 0 2px var(--canvas-bg);
    z-index: 3;
  }
  .ghost {
    position: absolute;
    pointer-events: none;
    transform: translate(12px, 12px);
    display: grid;
    gap: 2px;
    padding: 6px 10px;
    border-radius: 10px;
    background: var(--surface);
    color: var(--fg);
    border: 1px solid var(--focus);
    box-shadow: var(--panel-shadow);
    font-size: 0.8125rem;
    white-space: nowrap;
    z-index: 4;
  }
  .ghost.invalid {
    border-color: var(--danger);
  }
  .gl {
    font-weight: 600;
  }
  .gt {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .ghost.invalid .gt {
    color: var(--danger);
  }

  .palette {
    position: absolute;
    top: 10px;
    left: 50%;
    transform: translateX(-50%);
    max-width: calc(100% - 20px);
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    box-shadow: var(--panel-shadow);
    overflow-x: auto;
    z-index: 5;
  }
  .plabel {
    font-size: 0.75rem;
    color: var(--muted);
    padding: 0 6px;
    white-space: nowrap;
  }
  .pbtn {
    display: grid;
    justify-items: center;
    min-width: 56px;
    min-height: 40px;
    padding: 2px 8px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--fg);
    font: inherit;
    cursor: grab;
    touch-action: none;
    line-height: 1.25;
  }
  .pbtn:hover {
    border-color: var(--line);
  }
  .psep {
    align-self: stretch;
    width: 1px;
    margin: 4px 2px;
    background: var(--line);
  }
  .pk {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .pn {
    font-size: 0.625rem;
    color: var(--muted);
  }

  .zoom {
    position: absolute;
    left: 10px;
    bottom: 10px;
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    box-shadow: var(--panel-shadow);
    z-index: 5;
  }
  .zbtn {
    min-width: 36px;
    min-height: 36px;
    padding: 0 8px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--fg);
    font: inherit;
    font-size: 0.9375rem;
    cursor: pointer;
  }
  .zbtn:hover {
    background: var(--surface-2);
  }
  .zbtn.on {
    background: var(--accent-soft);
  }
  .zbtn.pct {
    min-width: 56px;
    font-variant-numeric: tabular-nums;
  }
  .zbtn.fit {
    font-size: 0.8125rem;
  }
  .help {
    position: absolute;
    right: 12px;
    bottom: 8px;
    margin: 0;
    font-size: 0.75rem;
    color: var(--muted);
    pointer-events: none;
  }
  .toast {
    position: absolute;
    left: 50%;
    bottom: 56px;
    transform: translateX(-50%);
    margin: 0;
    padding: 0;
    font-size: 0.875rem;
    color: var(--danger);
    background: var(--danger-soft);
    border-radius: 8px;
    pointer-events: none;
    z-index: 6;
  }
  .toast:not(:empty) {
    padding: 6px 12px;
  }
  @media (max-width: 800px) {
    .help {
      display: none;
    }
  }
</style>
