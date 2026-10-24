<script lang="ts">
  // 1 色の入力: トークン（@accent など）か #hex（カラー入力 + 不透明度 + 16 進の直接入力）。
  import { hexToRgbAlpha, rgbAlphaToHex } from '$lib/keyboard/look-color-edit'
  import { parseSolidColor } from '$lib/keyboard/look-color'
  import { LOOK_TOKENS } from '$lib/keyboard/types'

  interface Props {
    value: string | undefined
    label: string
    disabled?: boolean
    onchange: (value: string) => void
  }
  let { value, label, disabled = false, onchange }: Props = $props()

  const TOKEN_LABELS: Record<(typeof LOOK_TOKENS)[number], string> = {
    label: '@label（文字色）',
    secondaryLabel: '@secondaryLabel（薄い文字色）',
    key: '@key（通常キーの背景）',
    function: '@function（機能キーの背景）',
    accent: '@accent（アクセント）',
    separator: '@separator（区切り線）',
    clear: '@clear（透明）'
  }
  const token = $derived(typeof value === 'string' && value.startsWith('@') ? value : null)
  const parsed = $derived(hexToRgbAlpha(value))
  const valid = $derived(typeof value === 'string' && parseSolidColor(value) !== undefined)
  const rgb = $derived(parsed?.rgb ?? '#000000')
  const alpha = $derived(parsed?.alpha ?? 1)
  let badText = $state(false)

  function pickToken(v: string) {
    if (v === 'custom') onchange(rgbAlphaToHex(rgb, alpha))
    else onchange(v)
  }
</script>

<div class="solid" class:invalid={!valid && value !== undefined}>
  <select aria-label="{label}の種類" {disabled} value={token ?? 'custom'} onchange={(e) => pickToken(e.currentTarget.value)}>
    <option value="custom">色を指定（#hex）</option>
    {#each LOOK_TOKENS as t (t)}<option value="@{t}">{TOKEN_LABELS[t]}</option>{/each}
  </select>
  {#if !token}
    <input type="color" aria-label="{label}（色）" {disabled} value={rgb} oninput={(e) => onchange(rgbAlphaToHex(e.currentTarget.value, alpha))} />
    <label class="alpha">
      <span class="sr-only">{label}の不透明度</span>
      <input type="range" min="0" max="1" step="0.01" {disabled} value={alpha} oninput={(e) => onchange(rgbAlphaToHex(rgb, Number(e.currentTarget.value)))} />
      <span class="pct" aria-hidden="true">{Math.round(alpha * 100)}%</span>
    </label>
    <input
      class="hex"
      aria-label="{label}（#RGB / #RRGGBB / #RRGGBBAA）"
      aria-invalid={badText || (!valid && value !== undefined) ? 'true' : undefined}
      {disabled}
      value={value ?? ''}
      spellcheck="false"
      maxlength="9"
      placeholder="#RRGGBB"
      oninput={(e) => {
        const v = e.currentTarget.value
        badText = parseSolidColor(v) === undefined
        if (!badText) onchange(v)
      }}
      onblur={(e) => {
        badText = false
        e.currentTarget.value = value ?? ''
      }}
    />
  {/if}
</div>
{#if badText}<p class="err" role="alert">色は #RGB / #RRGGBB / #RRGGBBAA で入力してください</p>{/if}

<style>
  .solid {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  select {
    min-height: 28px;
    max-width: 100%;
  }
  input[type='color'] {
    width: 36px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: none;
  }
  .alpha {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .alpha input {
    width: 80px;
  }
  .pct {
    color: var(--muted);
    font-size: 0.8125rem;
    min-width: 2.6em;
  }
  .hex {
    width: 7.5em;
    min-height: 28px;
    box-sizing: border-box;
    font-family: var(--font-mono);
    font-size: 0.8125rem;
  }
  .err {
    margin: 2px 0 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  .invalid .hex,
  .hex[aria-invalid='true'] {
    border-color: var(--danger);
  }
</style>
