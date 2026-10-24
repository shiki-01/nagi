<script lang="ts">
  // スライダー + 数値欄（+ 単位）。範囲外・数値でない入力は書き込まず、その場でエラーを出す（クランプしない。仕様 §5.5）。
  interface Props {
    value: number | undefined
    min: number
    max: number
    step?: number
    unit?: string
    disabled?: boolean
    /** 読み上げ用の名前（スライダーと数値欄に付く）。 */
    label: string
    onchange: (value: number) => void
  }
  let { value, min, max, step, unit, disabled = false, label, onchange }: Props = $props()

  const span = $derived(max - min)
  const stepV = $derived(step ?? (span <= 2 ? 0.05 : span <= 50 ? 0.5 : span <= 100 ? 1 : 10))
  const outOfRange = $derived(value !== undefined && (!Number.isFinite(value) || value < min || value > max))
  let bad = $state<string | null>(null)
  const uid = $props.id()

  function commit(raw: string) {
    const n = Number(raw)
    if (raw.trim() === '' || !Number.isFinite(n)) {
      bad = '数値を入力してください'
      return
    }
    if (n < min || n > max) {
      bad = `${min}〜${max} の範囲で入力してください`
      return
    }
    bad = null
    onchange(Math.round(n * 1e4) / 1e4)
  }
</script>

<div class="num">
  <input type="range" aria-label="{label}（スライダー）" {min} {max} step={stepV} value={value ?? min} {disabled} oninput={(e) => commit(e.currentTarget.value)} />
  <input
    type="number"
    aria-label={label}
    {min}
    {max}
    step={stepV}
    value={value ?? ''}
    {disabled}
    aria-invalid={bad || outOfRange ? 'true' : undefined}
    aria-describedby={bad || outOfRange ? `${uid}-e` : undefined}
    oninput={(e) => commit(e.currentTarget.value)}
    onblur={(e) => {
      bad = null
      e.currentTarget.value = value === undefined ? '' : String(value)
    }}
  />
  {#if unit}<span class="unit">{unit}</span>{/if}
</div>
{#if bad}
  <p class="err" id="{uid}-e" role="alert">{bad}</p>
{:else if outOfRange}
  <p class="err" id="{uid}-e">範囲外の値です（{min}〜{max}）。このプロパティは無視されます</p>
{/if}

<style>
  .num {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  input[type='range'] {
    flex: 1 1 80px;
    min-width: 0;
  }
  input[type='number'] {
    flex: 0 0 5.5em;
    width: 5.5em;
    min-height: 28px;
    box-sizing: border-box;
  }
  .unit {
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .err {
    margin: 2px 0 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  input[aria-invalid='true'] {
    border-color: var(--danger);
  }
</style>
