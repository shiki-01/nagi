<script lang="ts">
  // プレビューの状態の切り替え。局所の状態（押下・離した直後・長押し・フリック中）は選択中のキー（無ければ全キー）、
  // 大域の状態（移動中・入力中・Shift・Caps）は全キーに適用して表示する。
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { LOOK_STATES, LOOK_STATE_INFO } from '$lib/keyboard/types'

  let { store }: { store: EditorStore } = $props()

  const on = $derived(new Set(store.previewStates))
</script>

<div class="bar" role="group" aria-label="プレビューの状態">
  <span class="ttl">プレビューの状態</span>
  {#each LOOK_STATES as s (s)}
    <label class="chk" title={LOOK_STATE_INFO[s].scope === 'local' ? '選択中のキー（無ければ全キー）に適用' : '全キーに適用'}>
      <input type="checkbox" checked={on.has(s)} onchange={(e) => store.togglePreviewState(s, e.currentTarget.checked)} />
      <span>{LOOK_STATE_INFO[s].label}</span>
    </label>
  {/each}
  <button type="button" class="btn small" disabled={on.size === 0} onclick={() => (store.previewStates = [])}>すべて解除</button>
  <label class="scheme">
    <span>外観</span>
    <select value={store.previewScheme} onchange={(e) => (store.previewScheme = e.currentTarget.value as 'auto' | 'light' | 'dark')}>
      <option value="auto">自動</option>
      <option value="light">ライト</option>
      <option value="dark">ダーク</option>
    </select>
  </label>
  <span class="note">実機とは細部が異なります</span>
</div>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--line);
    background: var(--surface);
    font-size: 0.8125rem;
  }
  .ttl {
    font-weight: 600;
  }
  .chk,
  .scheme {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .scheme select {
    min-height: 26px;
  }
  .note {
    color: var(--muted);
  }
</style>
