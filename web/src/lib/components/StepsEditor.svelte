<script lang="ts">
  import { KNOWN_ACTIONS, LIMITS } from '$lib/keyboard/types'
  import type { Step } from '$lib/keyboard/types'

  interface Props {
    steps: Step[]
    legend: string
    onchange: (steps: Step[], coalesce?: string) => void
    uid: string
  }
  let { steps, legend, onchange, uid }: Props = $props()

  const isAction = (s: Step): s is { action: string } => 'action' in s

  function set(i: number, step: Step, coalesce?: string) {
    onchange(steps.map((s, j) => (j === i ? step : s)), coalesce)
  }
  function move(i: number, d: -1 | 1) {
    const j = i + d
    if (j < 0 || j >= steps.length) return
    const next = [...steps]
    ;[next[i], next[j]] = [next[j], next[i]]
    onchange(next)
  }
</script>

<fieldset class="steps">
  <legend>{legend}</legend>
  <datalist id="actions-{uid}">
    {#each KNOWN_ACTIONS as a (a)}<option value={a}></option>{/each}
  </datalist>
  <ol>
    {#each steps as step, i (i)}
      <li>
        <select
          aria-label="手順 {i + 1} の種類"
          value={isAction(step) ? 'action' : 'text'}
          onchange={(e) => set(i, e.currentTarget.value === 'action' ? { action: 'space' } : { text: 'あ' })}
        >
          <option value="text">文字列を挿入</option>
          <option value="action">操作</option>
        </select>
        {#if isAction(step)}
          <input
            aria-label="手順 {i + 1} の操作名"
            list="actions-{uid}"
            value={step.action}
            oninput={(e) => set(i, { action: e.currentTarget.value }, `step${i}`)}
          />
        {:else}
          <input
            aria-label="手順 {i + 1} の文字列"
            maxlength={LIMITS.shortcutTextMax}
            value={step.text}
            oninput={(e) => set(i, { text: e.currentTarget.value }, `step${i}`)}
          />
        {/if}
        <span class="ctl">
          <button type="button" class="btn small" aria-label="手順 {i + 1} を上へ" disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
          <button type="button" class="btn small" aria-label="手順 {i + 1} を下へ" disabled={i === steps.length - 1} onclick={() => move(i, 1)}>↓</button>
          <button
            type="button"
            class="btn small danger"
            aria-label="手順 {i + 1} を削除"
            disabled={steps.length <= 1}
            onclick={() => onchange(steps.filter((_, j) => j !== i))}>削除</button
          >
        </span>
      </li>
    {/each}
  </ol>
  <button
    type="button"
    class="btn small"
    disabled={steps.length >= LIMITS.stepsMax}
    onclick={() => onchange([...steps, { text: '' }])}>手順を追加</button
  >
</fieldset>

<style>
  .steps {
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 8px 10px 10px;
    margin: 0;
    min-width: 0;
  }
  legend {
    font-size: 0.875rem;
    padding: 0 4px;
  }
  ol {
    list-style: none;
    margin: 0 0 8px;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  li {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }
  li input {
    flex: 1 1 120px;
    min-width: 0;
  }
  .ctl {
    display: flex;
    gap: 4px;
  }
</style>
