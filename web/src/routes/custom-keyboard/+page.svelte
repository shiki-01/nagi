<script lang="ts">
  import { onMount, tick } from 'svelte'
  import Meta from '$lib/components/Meta.svelte'
  import ErrorList from '$lib/components/ErrorList.svelte'
  import JsonPanel from '$lib/components/JsonPanel.svelte'
  import KeyForm from '$lib/components/KeyForm.svelte'
  import KeyboardCanvas from '$lib/components/KeyboardCanvas.svelte'
  import LayoutMetaForm from '$lib/components/LayoutMetaForm.svelte'
  import LookInspector from '$lib/components/LookInspector.svelte'
  import PreviewStateBar from '$lib/components/PreviewStateBar.svelte'
  import { jumpTarget } from '$lib/keyboard/look-jump'
  import { presetResult } from '$lib/keyboard/presets'
  import { EditorStore, loadInitialState } from '$lib/keyboard/store.svelte'

  // SSR / prerender 時は localStorage が無いので、初期値で作ってマウント後に保存済みの状態へ差し替える。
  let store = $state(new EditorStore(loadInitialState(presetResult.presets, undefined)))
  let ready = $state(false)
  let presetPick = $state(presetResult.presets[0]?.id ?? '')
  let drawer = $state<'json' | 'errors' | null>(null)
  let sheetOpen = $state(false)
  let canvas = $state<ReturnType<typeof KeyboardCanvas>>()

  const selCount = $derived(store.selectedIds.length)
  const errors = $derived(store.errors)
  const warnings = $derived(store.warnings)
  const lookTab = $derived(store.inspectorTab === 'look')
  const TABS = [
    { id: 'basic', label: '基本' },
    { id: 'look', label: '詳細設定' }
  ] as const

  onMount(() => {
    store = new EditorStore(loadInitialState(presetResult.presets, localStorage))
    ready = true
  })

  // 別のレイアウトに切り替わった / 追加されたときは全体を表示し直す（id の編集では動かさない）。
  let prevShape = ''
  $effect(() => {
    const shape = `${store.state.layouts.findIndex((l) => l.id === store.state.currentId)}/${store.state.layouts.length}`
    if (shape === prevShape) return
    prevShape = shape
    tick().then(() => canvas?.fit())
  })

  // 幅が狭いときのボトムシートは、キーを選んだら開く。
  $effect(() => {
    if (selCount > 0) sheetOpen = true
  })

  // 検証メッセージの場所を開く（キー・行・rules・全体の見た目）。詳細設定のものは該当のタブへ。
  function jump(path: string) {
    const t = jumpTarget(store.layout, path)
    if (!t) return
    store.applyJump(t)
    sheetOpen = true
  }
  const canJump = (path: string) => jumpTarget(store.layout, path) !== undefined

  function onTabKey(e: KeyboardEvent) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0
    if (d === 0) return
    e.preventDefault()
    store.inspectorTab = store.inspectorTab === 'basic' ? 'look' : 'basic'
    document.getElementById(`itab-${store.inspectorTab}`)?.focus()
  }

  function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return
    const mod = e.metaKey || e.ctrlKey
    if (!mod) return
    const k = e.key.toLowerCase()
    if (k === 'z') {
      e.preventDefault()
      if (e.shiftKey) store.redo()
      else store.undo()
    } else if (k === 'y') {
      e.preventDefault()
      store.redo()
    }
  }

  function removeLayout() {
    if (confirm(`レイアウト「${store.layout.name}」を削除します。よろしいですか？（元に戻すで復元できます）`)) store.remove()
  }
  function toggle(d: 'json' | 'errors') {
    drawer = drawer === d ? null : d
  }
</script>

<Meta
  title="キーボードレイアウト編集"
  path="/custom-keyboard"
  description="Nagi の iOS キーボードの配置とキーの動作を、ブラウザ上で編集して JSON に書き出します。"
/>
<svelte:window {onkeydown} />

<main class="editor">
  <h1 class="sr-only">キーボードレイアウト編集</h1>

  <div class="bar" role="toolbar" aria-label="レイアウトの管理">
    <label class="sel">
      <span class="sr-only">レイアウト</span>
      <select aria-label="編集するレイアウト" value={store.state.currentId} onchange={(e) => store.select(e.currentTarget.value)} disabled={!ready}>
        {#each store.state.layouts as l (l.id)}<option value={l.id}>{l.name}（{l.id}）</option>{/each}
      </select>
    </label>
    <button type="button" class="btn small" onclick={() => store.create()} disabled={!ready}>新規</button>
    <button type="button" class="btn small" onclick={() => store.duplicate()} disabled={!ready}>複製</button>
    <button type="button" class="btn small danger" onclick={removeLayout} disabled={!ready}>削除</button>
    <span class="sep" aria-hidden="true"></span>
    <button type="button" class="btn small" onclick={() => store.undo()} disabled={!store.canUndo} title="元に戻す（Ctrl/⌘+Z）">↶ 元に戻す</button>
    <button type="button" class="btn small" onclick={() => store.redo()} disabled={!store.canRedo} title="やり直す（Ctrl/⌘+Shift+Z）">↷ やり直す</button>
    <span class="sep" aria-hidden="true"></span>
    {#if presetResult.presets.length}
      <label class="sel preset">
        <span class="sr-only">プリセット</span>
        <select aria-label="追加するプリセット" bind:value={presetPick}>
          {#each presetResult.presets as p (p.id)}<option value={p.id}>{p.name}（{p.id}）</option>{/each}
        </select>
      </label>
      <button
        type="button"
        class="btn small"
        disabled={!ready}
        onclick={() => {
          const p = presetResult.presets.find((x) => x.id === presetPick)
          if (p) store.add(p)
        }}>プリセットを追加</button
      >
    {/if}
    <span class="grow"></span>
    <button type="button" class="btn small" aria-pressed={drawer === 'json'} aria-expanded={drawer === 'json'} onclick={() => toggle('json')}>JSON 入出力</button>
    <button
      type="button"
      class="btn small status"
      class:bad={errors.length > 0}
      aria-pressed={drawer === 'errors'}
      aria-expanded={drawer === 'errors'}
      onclick={() => toggle('errors')}
      >{errors.length ? `検証エラー ${errors.length} 件` : '検証: エラーなし'}{warnings.length ? `・警告 ${warnings.length} 件` : ''}</button
    >
  </div>

  {#if presetResult.fallback}
    <p class="notice" role="note">プリセットがまだ見つからないため、サンプルのレイアウトを表示しています。</p>
  {/if}
  {#if store.saveError}<p class="notice err" role="alert">{store.saveError}</p>{/if}

  <div class="body">
    <div class="stage">
      {#if lookTab}<PreviewStateBar {store} />{/if}
      <div class="canvaswrap">
        <KeyboardCanvas bind:this={canvas} {store} {ready} />
      </div>

      {#if drawer}
        <section class="drawer" aria-label={drawer === 'json' ? 'JSON の書き出しと読み込み' : '検証エラー'}>
          <button type="button" class="btn small close" onclick={() => (drawer = null)} aria-label="閉じる">✕ 閉じる</button>
          {#if drawer === 'json'}
            <p class="note">
              編集内容はこのブラウザの localStorage にだけ自動保存され、サーバーには送信されません。仕様は
              <code>docs/ios-keyboard-layout.md</code>（v0 draft）に基づきます。アプリ側の取り込み機能は開発中です。
            </p>
            <JsonPanel {store} />
          {:else}
            {#if errors.length}
              <ErrorList errors={[...errors, ...warnings]} onjump={jump} canjump={canJump} />
            {:else}
              <p class="valid" role="status">スキーマ検証: エラーなし</p>
              {#if warnings.length}<ErrorList errors={warnings} onjump={jump} canjump={canJump} />{/if}
            {/if}
            {#if presetResult.problems.length}
              <ErrorList errors={presetResult.problems.map((m) => ({ path: '', message: m }))} title="読み込めなかったプリセット" />
            {/if}
          {/if}
        </section>
      {/if}
    </div>

    <aside class="inspector" class:open={sheetOpen} aria-label="インスペクタ">
      <div class="ihead">
        <h2>インスペクタ{selCount > 1 ? `（${selCount} 個のキーを選択中）` : selCount === 1 ? '（キー）' : '（レイアウト）'}</h2>
        <button type="button" class="btn small toggle" aria-expanded={sheetOpen} onclick={() => (sheetOpen = !sheetOpen)}>{sheetOpen ? '閉じる ▾' : '詳細 ▴'}</button>
      </div>
      <div class="itabs" role="tablist" aria-label="インスペクタの種類" tabindex="-1" onkeydown={onTabKey}>
        {#each TABS as t (t.id)}
          <button
            type="button"
            role="tab"
            id="itab-{t.id}"
            class="itab"
            class:on={store.inspectorTab === t.id}
            aria-selected={store.inspectorTab === t.id}
            aria-controls="ipanel"
            tabindex={store.inspectorTab === t.id ? 0 : -1}
            onclick={() => (store.inspectorTab = t.id)}>{t.label}</button
          >
        {/each}
      </div>
      <div class="ibody" role="tabpanel" id="ipanel" aria-labelledby="itab-{store.inspectorTab}">
        {#if lookTab}
          <LookInspector {store} />
        {:else if selCount === 0}
          <LayoutMetaForm {store} />
        {:else if selCount === 1 && store.selectedKey}
          <KeyForm {store} />
        {:else}
          <div class="multi">
            <p class="hint">Shift / ⌘ クリックで選択を増減、ドラッグでまとめて移動できます。詳細を編集するには 1 つだけ選んでください。</p>
            <div class="btns">
              <button type="button" class="btn small danger" onclick={() => store.removeSelected()}>選択したキーを削除</button>
              <button type="button" class="btn small" onclick={() => (store.selectedIds = [])}>選択を解除</button>
            </div>
          </div>
        {/if}
      </div>
    </aside>
  </div>
</main>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    height: calc(100dvh - 58px);
    min-height: 560px;
    box-sizing: border-box;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding: 6px 10px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
  }
  .sel {
    display: block;
    min-width: 0;
    flex: 0 1 260px;
  }
  .sel select {
    width: 100%;
    min-height: 32px;
    padding: 2px 6px;
    font-size: 0.875rem;
    text-overflow: ellipsis;
  }
  .sel.preset {
    flex-basis: 200px;
  }
  .sep {
    width: 1px;
    align-self: stretch;
    background: var(--line);
    margin: 0 2px;
  }
  .grow {
    flex: 1 1 0;
  }
  .status {
    color: var(--ok);
  }
  .status.bad {
    color: var(--danger);
    border-color: var(--danger);
    font-weight: 600;
  }
  .notice {
    margin: 0;
    padding: 4px 12px;
    font-size: 0.8125rem;
    background: var(--accent-soft);
  }
  .notice.err {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .canvaswrap {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .drawer {
    position: absolute;
    left: 10px;
    top: 74px;
    z-index: 8;
    width: min(420px, calc(100% - 20px));
    max-height: calc(100% - 140px);
    overflow: auto;
    box-sizing: border-box;
    padding: 12px;
    display: grid;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    box-shadow: var(--panel-shadow);
  }
  .drawer .close {
    justify-self: end;
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  code {
    font-size: 0.875em;
    background: var(--surface-2);
    padding: 0 4px;
    border-radius: 4px;
  }
  .valid {
    margin: 0;
    color: var(--ok);
    font-size: 0.875rem;
  }
  .inspector {
    flex: 0 0 360px;
    width: 360px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border-left: 1px solid var(--line);
  }
  .ihead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
  }
  .ihead h2 {
    margin: 0;
    font-size: 0.9375rem;
  }
  .toggle {
    display: none;
  }
  .itabs {
    display: flex;
    border-bottom: 1px solid var(--line);
  }
  .itab {
    flex: 1;
    min-height: 36px;
    padding: 4px 8px;
    border: 0;
    border-bottom: 3px solid transparent;
    background: none;
    color: var(--muted);
    font: inherit;
    font-size: 0.9375rem;
    cursor: pointer;
  }
  .itab.on {
    color: var(--fg);
    font-weight: 700;
    border-bottom-color: var(--accent);
  }
  .ibody {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 12px;
  }
  .multi {
    display: grid;
    gap: 10px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.875rem;
  }
  .btns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  @media (max-width: 800px) {
    .editor {
      height: calc(100dvh - 100px);
    }
    /* 1 行に収めて横スクロール（縦の領域をキャンバスに回す）。 */
    .bar {
      flex-wrap: nowrap;
      overflow-x: auto;
    }
    .bar > :global(*) {
      flex-shrink: 0;
    }
    .sel {
      flex: 0 0 200px;
    }
    /* ボトムシート: キャンバスの下にドッキングし、開閉できる。 */
    .body {
      flex-direction: column;
    }
    .inspector {
      flex: none;
      width: auto;
      max-height: 42%;
      border-left: 0;
      border-top: 1px solid var(--line);
      border-radius: 16px 16px 0 0;
      box-shadow: var(--panel-shadow);
      z-index: 9;
    }
    .toggle {
      display: inline-flex;
    }
    .inspector:not(.open) .ibody,
    .inspector:not(.open) .itabs {
      display: none;
    }
    .drawer {
      top: 64px;
      max-height: calc(100% - 80px);
    }
  }
</style>
