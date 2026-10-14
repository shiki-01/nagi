<script lang="ts">
  import { onMount } from 'svelte'
  import Meta from '$lib/components/Meta.svelte'
  import ErrorList from '$lib/components/ErrorList.svelte'
  import JsonPanel from '$lib/components/JsonPanel.svelte'
  import KeyForm from '$lib/components/KeyForm.svelte'
  import KeyboardPreview from '$lib/components/KeyboardPreview.svelte'
  import StructureEditor from '$lib/components/StructureEditor.svelte'
  import { presetResult } from '$lib/keyboard/presets'
  import { EditorStore, loadInitialState } from '$lib/keyboard/store.svelte'
  import { LAYOUT_MODES, LIMITS } from '$lib/keyboard/types'
  import type { LayoutMode } from '$lib/keyboard/types'

  const MODE_LABELS: Record<LayoutMode, string> = {
    kana: 'kana（かな）',
    alpha: 'alpha（英字）',
    number: 'number（数字）',
    symbol: 'symbol（記号）',
    emoji: 'emoji（絵文字）',
    custom: 'custom'
  }

  // SSR / prerender 時は localStorage が無いので、初期値で作ってマウント後に保存済みの状態へ差し替える。
  let store = $state(new EditorStore(loadInitialState(presetResult.presets, undefined)))
  let ready = $state(false)
  let presetPick = $state(presetResult.presets[0]?.id ?? '')

  onMount(() => {
    store = new EditorStore(loadInitialState(presetResult.presets, localStorage))
    ready = true
  })

  function jump(path: string) {
    const m = /^rows\[(\d+)\]\.keys\[(\d+)\]/.exec(path)
    if (!m) return
    const key = store.layout.rows[Number(m[1])]?.keys[Number(m[2])]
    if (key) store.selectedKeyId = key.id
  }

  function onkeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return
    if (!(e.metaKey || e.ctrlKey) || e.key.toLowerCase() !== 'z') {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'y') {
        e.preventDefault()
        store.redo()
      }
      return
    }
    e.preventDefault()
    if (e.shiftKey) store.redo()
    else store.undo()
  }

  function removeLayout() {
    if (confirm(`レイアウト「${store.layout.name}」を削除します。よろしいですか？（元に戻すで復元できます）`)) store.remove()
  }
</script>

<Meta
  title="キーボードレイアウト編集"
  path="/custom-keyboard"
  description="Nagi の iOS キーボードの配置とキーの動作を、ブラウザ上で編集して JSON に書き出します。"
/>
<svelte:window {onkeydown} />

<main class="wrap">
  <h1>キーボードレイアウト編集</h1>
  <p class="intro">
    キーの配置と動作をブラウザ上で編集し、JSON として書き出せます。編集内容はこのブラウザの localStorage にだけ自動保存され、
    サーバーには送信されません。仕様は <code>docs/ios-keyboard-layout.md</code>（v0 draft）に基づきます。
    アプリ側の取り込み機能は開発中です。
  </p>

  {#if presetResult.fallback}
    <p class="notice" role="note">
      プリセットがまだ見つからないため、サンプルのレイアウトを表示しています。
    </p>
  {/if}
  {#if presetResult.problems.length}
    <ErrorList errors={presetResult.problems.map((m) => ({ path: '', message: m }))} title="読み込めなかったプリセット" />
  {/if}

  <div class="toolbar" role="toolbar" aria-label="レイアウトの管理">
    <label class="sel">
      <span>レイアウト</span>
      <select value={store.state.currentId} onchange={(e) => store.select(e.currentTarget.value)} disabled={!ready}>
        {#each store.state.layouts as l (l.id)}<option value={l.id}>{l.name}（{l.id}）</option>{/each}
      </select>
    </label>
    <button type="button" class="btn" onclick={() => store.create()} disabled={!ready}>新規</button>
    <button type="button" class="btn" onclick={() => store.duplicate()} disabled={!ready}>複製</button>
    <button type="button" class="btn danger" onclick={removeLayout} disabled={!ready}>削除</button>
    <span class="sep" aria-hidden="true"></span>
    <button type="button" class="btn" onclick={() => store.undo()} disabled={!store.canUndo}>元に戻す</button>
    <button type="button" class="btn" onclick={() => store.redo()} disabled={!store.canRedo}>やり直す</button>
  </div>

  {#if presetResult.presets.length}
    <div class="toolbar presets">
      <label class="sel">
        <span>プリセットから追加</span>
        <select bind:value={presetPick}>
          {#each presetResult.presets as p (p.id)}<option value={p.id}>{p.name}（{p.id}）</option>{/each}
        </select>
      </label>
      <button
        type="button"
        class="btn"
        disabled={!ready}
        onclick={() => {
          const p = presetResult.presets.find((x) => x.id === presetPick)
          if (p) store.add(p)
        }}>追加</button
      >
    </div>
  {/if}

  {#if store.saveError}<p class="err" role="alert">{store.saveError}</p>{/if}

  <div class="cols">
    <div class="col-a">
      <section class="card" aria-labelledby="meta-h">
        <h2 id="meta-h">レイアウトの情報</h2>
        <div class="metagrid">
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
      </section>

      <section class="card" aria-labelledby="prev-h">
        <h2 id="prev-h">プレビュー</h2>
        <KeyboardPreview layout={store.layout} selectedKeyId={store.selectedKeyId} onselect={(id) => (store.selectedKeyId = id)} />
        <p class="hint">フリックのキーは、4 方向の文字を小さく表示します。右上の点は長押しメニューがあるキーです。</p>
      </section>

      <section class="card">
        <StructureEditor {store} />
      </section>
    </div>

    <div class="col-b">
      {#if store.errors.length}
        <ErrorList errors={store.errors} onjump={jump} />
      {:else}
        <p class="valid" role="status">スキーマ検証: エラーなし</p>
      {/if}
      <section class="card" aria-label="キーの設定">
        <KeyForm {store} />
      </section>
      <section class="card">
        <JsonPanel {store} />
      </section>
    </div>
  </div>
</main>

<style>
  .wrap {
    max-width: 1200px;
    margin: 0 auto;
    padding: 16px;
    box-sizing: border-box;
  }
  h1 {
    margin: 8px 0;
    font-size: 1.75rem;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 1.0625rem;
  }
  .intro {
    color: var(--muted);
    max-width: 52em;
    font-size: 0.9375rem;
  }
  code {
    font-size: 0.875em;
    background: var(--surface-2);
    padding: 0 4px;
    border-radius: 4px;
  }
  .notice {
    background: var(--accent-soft);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 0.875rem;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: flex-end;
    margin: 12px 0;
  }
  .sel {
    display: grid;
    gap: 2px;
    font-size: 0.875rem;
    color: var(--muted);
    flex: 1 1 220px;
    min-width: 0;
  }
  .sel select {
    color: var(--fg);
  }
  .sep {
    flex: 0 0 8px;
  }
  .cols {
    display: grid;
    gap: 16px;
    grid-template-columns: minmax(0, 1fr);
  }
  .col-a,
  .col-b {
    display: grid;
    gap: 16px;
    align-content: start;
    min-width: 0;
  }
  @media (min-width: 980px) {
    .cols {
      grid-template-columns: minmax(0, 5fr) minmax(0, 6fr);
      align-items: start;
    }
    .col-a > section:nth-child(2) {
      position: sticky;
      top: 64px;
      z-index: 2;
    }
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 14px;
    min-width: 0;
  }
  .metagrid {
    display: grid;
    gap: 10px;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 200px), 1fr));
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
    color: var(--muted);
    font-size: 0.8125rem;
    margin: 8px 0 0;
  }
  .valid {
    margin: 0;
    color: var(--ok);
    font-size: 0.875rem;
  }
  .err {
    color: var(--danger);
  }
</style>
