<script lang="ts">
  // インスペクタの「詳細設定」タブ。全体 / 行 / キー の見た目と、状態（押下・長押しなど）ごとの見た目を編集する。
  // 編集内容はすべて store.layout（JSON の実体）に書き、フォームは常にそこから導出する（JSON との双方向同期）。
  import { findKey, keysAt } from '$lib/keyboard/ops'
  import { spaceDragOrigin } from '$lib/keyboard/look-css'
  import { hasAnyLook, readHoldMs } from '$lib/keyboard/look-ops'
  import type { InheritContext, LookLoc } from '$lib/keyboard/look-ops'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { BACKGROUND_PROPS, KEY_ROLES, LOOK_LIMITS, LOOK_STATES, LOOK_STATE_INFO, METRICS_PROPS, TOOLBAR_ROW } from '$lib/keyboard/types'
  import type { Key, KeyRole, StateName } from '$lib/keyboard/types'
  import ImageAssets from './ImageAssets.svelte'
  import LookPropsEditor from './LookPropsEditor.svelte'
  import NumberField from './NumberField.svelte'
  import RulesEditor from './RulesEditor.svelte'

  let { store }: { store: EditorStore } = $props()

  type Tab = 'normal' | StateName
  const TABS: { id: Tab; label: string }[] = [
    { id: 'normal', label: '通常' },
    ...(['pressed', 'released', 'longPress', 'spaceDrag', 'flicking', 'composing', 'shift', 'capsLock'] as const).map((id) => ({
      id,
      label: LOOK_STATE_INFO[id].label
    }))
  ]
  const STATE_HELP: Record<StateName, string> = {
    pressed: '指がそのキーに触れている間（フリック中・長押し中・スペースのドラッグ中も続きます）。',
    released: '指を離した直後の短い間（既定 200ms。保持時間で変えられます）。システムにタッチを奪われた取り消しでは入りません。',
    longPress: '長押しと判定されたとき（長押しメニューのあるキー、⌫ の連打が始まる瞬間）。それ以外のキーでは入りません。',
    flicking: 'フリックキーで、移動量がしきい値を超え、取り消しの範囲の外にいる間。',
    spaceDrag: 'スペースを左右にドラッグしてカーソルを動かしている間。全キーに同時に入ります。',
    shift: 'Shift が 1 回だけオンの間（全キー）。',
    capsLock: 'Caps Lock がオンの間（全キー）。',
    composing: '入力中の文字列（未確定）がある間（全キー）。'
  }

  const layout = $derived(store.layout)
  const ids = $derived(store.selectedIds)
  const scope = $derived(store.lookScope === 'key' && ids.length === 0 ? 'layout' : store.lookScope)
  /** 編集する行の番号。-1 はツールバー（layout.toolbar がある間だけ）。 */
  const rowIndex = $derived(
    store.lookRow === TOOLBAR_ROW && layout.toolbar ? TOOLBAR_ROW : Math.min(Math.max(store.lookRow, 0), Math.max(layout.rows.length - 1, 0))
  )
  const rowName = $derived(rowIndex === TOOLBAR_ROW ? 'ツールバー' : `${rowIndex + 1} 行目`)
  /** 行には状態ごとの指定が無いので、行のときは常に「通常」。 */
  const tab = $derived<Tab>(scope === 'row' ? 'normal' : store.lookState)
  const stateTab = $derived(tab === 'normal' ? null : tab)

  // キャンバスの選択に追従する: 選び始めたら「キー」、選択が無くなったら「全体」へ（手動の切り替えは次の選択の増減まで保つ）
  let prevCount = 0
  $effect(() => {
    const n = store.selectedIds.length
    if (n > 0 && prevCount === 0) store.lookScope = 'key'
    else if (n === 0 && prevCount > 0 && store.lookScope === 'key') store.lookScope = 'layout'
    prevCount = n
  })

  const firstKeyOf = (keys: readonly Key[]) => keys.find((k) => k.kind !== 'spacer')
  const locate = (pick: (k: Key) => boolean): { rowIndex: number; key: Key } | undefined => {
    for (const [ri, row] of layout.rows.entries()) {
      const key = row.keys.find((k) => k.kind !== 'spacer' && pick(k))
      if (key) return { rowIndex: ri, key }
    }
    return undefined
  }
  const prefersDark = () => typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches
  const scheme = $derived(store.previewScheme === 'auto' ? (prefersDark() ? 'dark' : 'light') : store.previewScheme)

  /** 継承される値を出すための代表キー（選択中の先頭・行の先頭・全体の先頭）。 */
  function ctxFor(found: { rowIndex: number; key: Key } | undefined, states: readonly StateName[]): InheritContext | undefined {
    return found ? { ...found, states, origin: spaceDragOrigin(layout), scheme } : undefined
  }
  const states = $derived<StateName[]>(stateTab ? [stateTab] : [])
  const ctx = $derived.by(() => {
    if (scope === 'key') {
      const f = findKey(layout, ids[0])
      return ctxFor(f ? { rowIndex: f.row, key: f.key } : undefined, states)
    }
    if (scope === 'row') {
      const key = firstKeyOf(keysAt(layout, rowIndex) ?? [])
      return ctxFor(key ? { rowIndex, key } : undefined, states)
    }
    return ctxFor(locate(() => true), states)
  })
  const roleCtx = (role: KeyRole) => ctxFor(locate((k) => (k.role ?? 'normal') === role) ?? locate(() => true), [])

  const keyLocs = $derived<LookLoc[]>(
    ids.map((keyId) => (stateTab ? { kind: 'keyState' as const, keyId, state: stateTab } : { kind: 'key' as const, keyId }))
  )
  const warnings = $derived(store.warnings)
  const anyLook = $derived(hasAnyLook(layout))
  let roleTab = $state<KeyRole | 'all'>('all')
  const uid = $props.id()

  const ROLE_TABS = ['all', ...KEY_ROLES] as const
  const SCOPES = [
    { id: 'layout', label: '全体' },
    { id: 'row', label: '行' },
    { id: 'key', label: 'キー' }
  ] as const

  function pickScope(id: 'layout' | 'row' | 'key') {
    if (id === 'key' && ids.length === 0) return
    if (id === 'row' && ids.length > 0) {
      const f = findKey(layout, ids[0])
      if (f) store.lookRow = f.row
    }
    store.lookScope = id
  }

  function tabKeydown(e: KeyboardEvent) {
    const i = TABS.findIndex((t) => t.id === tab)
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0
    if (d === 0 || scope === 'row') return
    e.preventDefault()
    const next = TABS[(i + d + TABS.length) % TABS.length]
    store.selectLookState(next.id)
    document.getElementById(`${uid}-tab-${next.id}`)?.focus()
  }

  function resetAll() {
    if (confirm('このレイアウトの見た目の指定（全体・役割・行・キー・状態・間隔・背景・画像）をすべて消します。よろしいですか？（元に戻すで復元できます）')) store.resetAllLooks()
  }
  function resetHere() {
    if (scope === 'key') {
      if (stateTab) store.resetLooks(keyLocs)
      else store.resetKeyLooks(ids)
    } else if (scope === 'row') store.resetLooks([{ kind: 'row', row: rowIndex }])
    else if (stateTab) store.removeRulesFor(stateTab)
    else store.resetLooks([{ kind: 'layout' }, ...KEY_ROLES.map((role) => ({ kind: 'role' as const, role }))])
  }
  const resetLabel = $derived(
    scope === 'key'
      ? stateTab
        ? `選択キーの「${LOOK_STATE_INFO[stateTab].label}」の指定を消す`
        : `選択キー（${ids.length} 個）の個別の見た目をすべてリセット（状態ごとの指定を含む）`
      : scope === 'row'
        ? `${rowName}の見た目をリセット`
        : stateTab
          ? `全体の「${LOOK_STATE_INFO[stateTab].label}」の指定をすべて消す`
          : '全キー共通・役割ごとの見た目をリセット'
  )
</script>

<section class="li" aria-label="詳細設定（見た目）">
  <div class="scope" role="radiogroup" aria-label="編集する範囲">
    {#each SCOPES as s (s.id)}
      <button
        type="button"
        role="radio"
        class="seg"
        aria-checked={scope === s.id}
        class:on={scope === s.id}
        disabled={s.id === 'key' && ids.length === 0}
        title={s.id === 'key' && ids.length === 0 ? 'キャンバスでキーを選ぶと編集できます' : undefined}
        onclick={() => pickScope(s.id)}>{s.label}</button
      >
    {/each}
  </div>
  {#if scope === 'row'}
    <label class="field">
      <span>編集する行</span>
      <select value={rowIndex} onchange={(e) => (store.lookRow = Number(e.currentTarget.value))}>
        {#if layout.toolbar}<option value={TOOLBAR_ROW}>ツールバー</option>{/if}
        {#each layout.rows as _, i (i)}<option value={i}>{i + 1} 行目</option>{/each}
      </select>
    </label>
    <p class="hint">
      その行のすべてのキーに掛かる見た目です。行には状態ごとの指定はありません（全体の状態タブの「対象」を使います）。{#if rowIndex === TOOLBAR_ROW}ツールバーのキーは、既定で文字 14pt・内側の余白（inset）6 が重なります。{/if}
    </p>
  {:else if scope === 'key'}
    <p class="hint">
      {ids.length === 1 ? `キー「${ids[0]}」` : `${ids.length} 個のキー（${ids.slice(0, 4).join('、')}${ids.length > 4 ? ' …' : ''}）`}の個別の見た目です。
      {#if ids.length > 1}編集すると選択した全部に同じ値を書きます。{/if}
    </p>
  {:else}
    <p class="hint">レイアウト全体の見た目です。キーを選ぶと、そのキーの個別の指定を編集できます。</p>
  {/if}

  <div class="tabs" role="tablist" aria-label="状態" tabindex="-1" onkeydown={tabKeydown}>
    {#each TABS as t (t.id)}
      <button
        type="button"
        role="tab"
        id="{uid}-tab-{t.id}"
        class="tab"
        class:on={tab === t.id}
        aria-selected={tab === t.id}
        aria-controls="{uid}-panel"
        tabindex={tab === t.id ? 0 : -1}
        disabled={scope === 'row' && t.id !== 'normal'}
        onclick={() => store.selectLookState(t.id)}>{t.label}</button
      >
    {/each}
  </div>

  <div class="panel" role="tabpanel" id="{uid}-panel" aria-labelledby="{uid}-tab-{tab}">
    {#if stateTab}
      <p class="hint">{STATE_HELP[stateTab]}</p>
    {/if}

    {#if scope === 'layout' && !stateTab}
      <details class="sec" open>
        <summary>全キー共通の見た目</summary>
        <div class="roles" role="radiogroup" aria-label="対象の役割">
          {#each ROLE_TABS as r (r)}
            <button type="button" role="radio" class="seg small" class:on={roleTab === r} aria-checked={roleTab === r} onclick={() => (roleTab = r)}
              >{r === 'all' ? '全キー' : r}</button
            >
          {/each}
        </div>
        {#if roleTab === 'all'}
          <p class="hint">役割（normal / function / accent）を問わず全キーに掛かります。役割ごとの指定はこれを上書きします。</p>
          <LookPropsEditor {store} locs={[{ kind: 'layout' }]} {ctx} />
        {:else}
          <p class="hint">role が {roleTab} のキーだけに掛かります。</p>
          <LookPropsEditor {store} locs={[{ kind: 'role', role: roleTab }]} ctx={roleCtx(roleTab)} />
        {/if}
      </details>

      <details class="sec">
        <summary>キー間隔・外周の余白 (metrics)</summary>
        {#each Object.entries(METRICS_PROPS) as [prop, r] (prop)}
          {@const label = prop === 'keySpacing' ? 'キー間の隙間' : prop === 'rowSpacing' ? '行間の隙間' : '外周の余白'}
          {@const cur = (layout.metrics as Record<string, unknown> | undefined)?.[prop]}
          <div class="mrow">
            <label class="use">
              <input
                type="checkbox"
                checked={cur !== undefined}
                onchange={(e) => store.setMetric(prop as 'keySpacing', e.currentTarget.checked ? 6 : undefined)}
              />
              <span>{label}</span>
            </label>
            {#if cur !== undefined}
              <NumberField value={typeof cur === 'number' ? cur : undefined} min={r.min} max={r.max} unit="pt" {label} onchange={(v) => store.setMetric(prop as 'keySpacing', v)} />
            {:else}
              <span class="hint">設定の既定値（キー間隔設定、外周は 6pt）</span>
            {/if}
          </div>
        {/each}
        <p class="hint">
          指定すると、設定アプリのキー間隔より優先されます。注意: 間隔を大きくしても行の高さは縮まないため、行数が多いレイアウトで大きくすると、キーボードの領域を超えることがあります。
        </p>
      </details>

      <details class="sec">
        <summary>キーボード全体の背景 (background)</summary>
        <p class="hint">キー領域の最背面に敷きます（候補バーの部分は不透明なバーなので描かれません）。キーの背景色を @clear や低い不透明度にすると透けて見えます。</p>
        <LookPropsEditor {store} locs={[{ kind: 'background' }]} only={BACKGROUND_PROPS} />
      </details>

      <details class="sec" open={(layout.assets && Object.keys(layout.assets).length > 0) || undefined}>
        <summary>画像 (assets)</summary>
        <ImageAssets {store} />
      </details>
    {:else if scope === 'layout' && stateTab}
      <RulesEditor {store} when={stateTab} {ctx} />
    {:else if scope === 'row'}
      <LookPropsEditor {store} locs={[{ kind: 'row', row: rowIndex }]} {ctx} />
    {:else if scope === 'key'}
      {#if stateTab === 'released'}
        {@const hold = readHoldMs(layout, keyLocs[0])}
        <div class="mrow">
          <label class="use">
            <input type="checkbox" checked={hold !== undefined} onchange={(e) => store.setHoldMs(keyLocs, e.currentTarget.checked ? LOOK_LIMITS.holdMsDefault : undefined)} />
            <span>戻るまでの時間 (holdMs)</span>
          </label>
          {#if hold !== undefined}
            <NumberField
              value={typeof hold === 'number' ? hold : undefined}
              min={LOOK_LIMITS.holdMsMin}
              max={LOOK_LIMITS.holdMsMax}
              step={10}
              unit="ms"
              label="戻るまでの時間"
              onchange={(v) => store.setHoldMs(keyLocs, v)}
            />
          {:else}
            <span class="hint">既定 {LOOK_LIMITS.holdMsDefault} ms</span>
          {/if}
        </div>
      {/if}
      <LookPropsEditor {store} locs={keyLocs} {ctx} allowContent={!!stateTab} />
    {/if}
  </div>

  <div class="resets">
    <button type="button" class="btn small" onclick={resetHere}>{resetLabel}</button>
    <button type="button" class="btn small danger" disabled={!anyLook} onclick={resetAll}>すべての見た目をリセット</button>
  </div>

  <p class="status" role="status">
    {#if warnings.length}警告 {warnings.length} 件（無視される値があります）。上部の「検証」で場所を確認できます。{:else}見た目の警告はありません。{/if}
  </p>
</section>

<style>
  .li {
    display: grid;
    gap: 10px;
  }
  .scope,
  .roles {
    display: flex;
    gap: 0;
  }
  .seg {
    flex: 1;
    min-height: 30px;
    padding: 2px 10px;
    border: 1px solid var(--line);
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    font-size: 0.875rem;
    cursor: pointer;
  }
  .seg + .seg {
    border-left: 0;
  }
  .seg:first-child {
    border-radius: 8px 0 0 8px;
  }
  .seg:last-child {
    border-radius: 0 8px 8px 0;
  }
  .seg.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }
  .seg:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .seg.small {
    font-size: 0.8125rem;
    min-height: 26px;
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tab {
    min-height: 28px;
    padding: 2px 10px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
    color: var(--fg);
    font: inherit;
    font-size: 0.8125rem;
    cursor: pointer;
  }
  .tab.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    font-weight: 600;
  }
  .tab:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .panel {
    display: grid;
    gap: 8px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .field {
    display: grid;
    gap: 2px;
    font-size: 0.875rem;
  }
  .field span {
    color: var(--muted);
  }
  .sec {
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 2px 10px 10px;
    display: grid;
    gap: 8px;
  }
  .sec > summary {
    cursor: pointer;
    padding: 8px 0;
    font-weight: 700;
    font-size: 0.9375rem;
  }
  .mrow {
    display: grid;
    gap: 4px;
    padding: 4px 0;
    font-size: 0.875rem;
  }
  .use {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .resets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .status {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
</style>
