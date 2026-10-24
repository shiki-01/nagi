<script lang="ts">
  // 見た目のプロパティ 1 つの入力行。左のチェックボックス＝「指定する」（外すと省略＝下位を継承）。
  // 外れている間は、解決結果（継承される値）を薄く表示する。右端の「リセット」でその指定を消す。
  import { LOOK_INITIAL, inheritedValue, readLook } from '$lib/keyboard/look-ops'
  import type { InheritContext, LookLoc } from '$lib/keyboard/look-ops'
  import { checkLookValue } from '$lib/keyboard/look-resolve'
  import { mainLabel } from '$lib/keyboard/key-display'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { LOOK_LIMITS, LOOK_PROPS } from '$lib/keyboard/types'
  import type { Color } from '$lib/keyboard/types'
  import ColorField from './ColorField.svelte'
  import IconField from './IconField.svelte'
  import NumberField from './NumberField.svelte'

  interface Props {
    store: EditorStore
    /** LOOK_PROPS のプロパティ名。 */
    prop: string
    label: string
    hint?: string
    /** 書き込み先（複数キーを選んでいるときは複数）。 */
    locs: readonly LookLoc[]
    /** 継承される値の計算に使う代表キーと状態。 */
    ctx?: InheritContext
    /** enum の選択肢の表示名。 */
    optionLabels?: Readonly<Record<string, string>>
  }
  let { store, prop, label, hint, locs, ctx, optionLabels = {} }: Props = $props()

  const UNITS: Record<string, string> = {
    fontSize: 'pt',
    cornerRadius: 'pt',
    borderWidth: 'pt',
    shadowRadius: 'pt',
    shadowX: 'pt',
    shadowY: 'pt',
    offsetX: 'pt',
    offsetY: 'pt',
    inset: 'pt',
    padding: 'pt',
    animationMs: 'ms',
    scale: '倍'
  }

  const spec = $derived(LOOK_PROPS[prop])
  const layout = $derived(store.layout)
  const first = $derived(locs[0])
  const own = $derived(first ? readLook(layout, first)[prop] : undefined)
  const isSet = $derived(first ? Object.prototype.hasOwnProperty.call(readLook(layout, first), prop) : false)
  const mixed = $derived(locs.length > 1 && locs.some((l) => JSON.stringify(readLook(layout, l)[prop]) !== JSON.stringify(own)))
  const inherited = $derived(first && ctx ? inheritedValue(layout, first, prop, ctx) : undefined)
  const problem = $derived.by(() => {
    if (!isSet || spec.type === 'number') return undefined // 数値は NumberField が出す
    const r = checkLookValue(prop, own)
    return r.ok ? undefined : r.reason
  })
  const assetIds = $derived(Object.keys(layout.assets && typeof layout.assets === 'object' ? layout.assets : {}))
  const uid = $props.id()

  const show = (v: string | number | undefined): string => {
    if (v === undefined) return '既定'
    if (typeof v === 'number') return `${v}${UNITS[prop] ? ` ${UNITS[prop]}` : ''}`
    return optionLabels[v] ?? v
  }

  function initial(): unknown {
    if (inherited !== undefined) return inherited
    if (spec.type === 'label') return ctx ? mainLabel(ctx.key) || '●' : '●'
    if (spec.type === 'icon') return 'sf:circle'
    if (spec.type === 'imageRef') return assetIds[0] ?? 'none'
    return LOOK_INITIAL[prop]
  }
  function toggle(on: boolean) {
    if (on) store.setLookProp(locs, prop, initial())
    else store.clearLookProp(locs, prop)
  }
  const set = (v: unknown, coalesce = false) => store.setLookProp(locs, prop, v, coalesce)

  const colorFallback = $derived(typeof inherited === 'string' ? inherited : String(LOOK_INITIAL[prop] ?? '#000000'))
  const colorValue = $derived<Color | undefined>(
    typeof own === 'string' || (typeof own === 'object' && own !== null && !Array.isArray(own)) ? (own as Color) : undefined
  )
  const enumValue = $derived(typeof own === 'string' ? own : '')
</script>

<div class="lf" class:off={!isSet} role="group" aria-labelledby="{uid}-n">
  <div class="head">
    <label class="use">
      <input type="checkbox" checked={isSet} disabled={locs.length === 0} onchange={(e) => toggle(e.currentTarget.checked)} />
      <span class="name" id="{uid}-n">{label}</span>
    </label>
    {#if isSet}
      <button type="button" class="btn small" aria-label="{label}をリセット（指定を消す）" onclick={() => store.clearLookProp(locs, prop)}>リセット</button>
    {/if}
  </div>
  {#if isSet}
    <div class="ctl">
      {#if spec.type === 'number'}
        <NumberField value={typeof own === 'number' ? own : undefined} min={spec.min} max={spec.max} unit={UNITS[prop]} {label} onchange={(v) => set(v, true)} />
      {:else if spec.type === 'enum'}
        <select aria-label={label} value={enumValue} onchange={(e) => set(e.currentTarget.value)}>
          {#if !spec.values.includes(enumValue)}<option value={enumValue} disabled>（不正な値: {String(own)}）</option>{/if}
          {#each spec.values as v (v)}<option value={v}>{optionLabels[v] ?? v}</option>{/each}
        </select>
      {:else if spec.type === 'color'}
        <ColorField value={colorValue} fallback={colorFallback} {label} onchange={(c) => set(c, true)} />
      {:else if spec.type === 'imageRef'}
        <select aria-label={label} value={enumValue} onchange={(e) => set(e.currentTarget.value)}>
          <option value="none">none（下位で指定された画像を外す）</option>
          {#each assetIds as id (id)}<option value={id}>{id}</option>{/each}
          {#if enumValue !== '' && enumValue !== 'none' && !assetIds.includes(enumValue)}
            <option value={enumValue} disabled>（画像が無い id: {enumValue}）</option>
          {/if}
        </select>
        {#if assetIds.length === 0}<p class="note">画像がありません。「全体」の「画像」で追加してください。</p>{/if}
      {:else if spec.type === 'label'}
        <input
          aria-label={label}
          value={typeof own === 'string' ? own : ''}
          maxlength={LOOK_LIMITS.labelMax}
          aria-invalid={problem ? 'true' : undefined}
          oninput={(e) => {
            if (e.currentTarget.value !== '') set(e.currentTarget.value, true)
          }}
        />
      {:else if spec.type === 'icon'}
        <IconField value={typeof own === 'string' ? own : undefined} {label} onchange={(v) => set(v, true)} />
      {/if}
      {#if problem}<p class="err">{problem}。このプロパティは無視されます</p>{/if}
      {#if mixed}<p class="note">選択したキーで値が異なります。編集すると全部に同じ値を書きます</p>{/if}
    </div>
  {:else}
    <p class="inh">継承: {show(inherited)}</p>
  {/if}
  {#if hint}<p class="hint">{hint}</p>{/if}
</div>

<style>
  .lf {
    display: grid;
    gap: 4px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
    font-size: 0.875rem;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
  }
  .use {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .off .name {
    color: var(--muted);
  }
  .ctl {
    display: grid;
    gap: 2px;
    min-width: 0;
    padding-left: 22px;
  }
  select,
  input:not([type]) {
    width: 100%;
    min-height: 28px;
    box-sizing: border-box;
  }
  .inh,
  .hint,
  .note {
    margin: 0;
    padding-left: 22px;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .hint {
    font-size: 0.75rem;
  }
  .ctl .note {
    padding-left: 0;
  }
  .err {
    margin: 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  input[aria-invalid='true'] {
    border-color: var(--danger);
  }
</style>
