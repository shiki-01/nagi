<script lang="ts">
  // 色の入力（Color = 単色 | { light, dark }）。「ライト／ダークを分ける」で 2 つの色入力にする。
  // 片方の外観だけを指定することもできる（書いていない側では未指定扱い。仕様 §2.2）。
  import { colorMode, mergeColor, splitColor } from '$lib/keyboard/look-color-edit'
  import type { Color } from '$lib/keyboard/types'
  import SolidColorInput from './SolidColorInput.svelte'

  interface Props {
    /** 指定されている値（無い・不正な型は undefined）。 */
    value: Color | undefined
    /** 指定が無いときに表示・初期値にする色（継承される値）。 */
    fallback: string
    label: string
    disabled?: boolean
    onchange: (value: Color) => void
  }
  let { value, fallback, label, disabled = false, onchange }: Props = $props()

  const mode = $derived(colorMode(value))
  const pair = $derived(typeof value === 'object' && value !== null ? value : undefined)
  const SIDES = ['light', 'dark'] as const
  const uid = $props.id()

  function toggleSplit(split: boolean) {
    onchange(split ? splitColor(value, fallback) : mergeColor(value, fallback))
  }
  function side(which: 'light' | 'dark', c: string) {
    onchange({ ...pair, [which]: c })
  }
  function toggleSide(which: 'light' | 'dark', on: boolean) {
    const next = { ...pair }
    if (on) next[which] = (which === 'light' ? pair?.dark : pair?.light) ?? fallback
    else delete next[which]
    onchange(next)
  }
</script>

<div class="color">
  {#if mode === 'single'}
    <SolidColorInput value={typeof value === 'string' ? value : disabled ? fallback : undefined} {label} {disabled} onchange={(c) => onchange(c)} />
  {:else}
    {#each SIDES as which (which)}
      {@const name = which === 'light' ? 'ライト' : 'ダーク'}
      {@const other = which === 'light' ? 'dark' : 'light'}
      <div class="side">
        <label class="use">
          <input
            type="checkbox"
            checked={pair?.[which] !== undefined}
            disabled={disabled || (pair?.[which] !== undefined && pair?.[other] === undefined)}
            onchange={(e) => toggleSide(which, e.currentTarget.checked)}
          />
          <span>{name}</span>
        </label>
        {#if pair?.[which] !== undefined}
          <SolidColorInput value={pair[which]} label="{label}（{name}）" {disabled} onchange={(c) => side(which, c)} />
        {:else}
          <span class="none">指定しない（下位の値）</span>
        {/if}
      </div>
    {/each}
  {/if}
  <label class="split" for="{uid}-split">
    <input id="{uid}-split" type="checkbox" checked={mode === 'pair'} {disabled} onchange={(e) => toggleSplit(e.currentTarget.checked)} />
    <span>ライト／ダークを分ける</span>
  </label>
</div>

<style>
  .color {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .side {
    display: grid;
    gap: 4px;
    padding-left: 8px;
    border-left: 3px solid var(--line);
  }
  .use,
  .split {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8125rem;
  }
  .none {
    color: var(--muted);
    font-size: 0.8125rem;
  }
</style>
