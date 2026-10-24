<script lang="ts">
  // 全体の状態の指定（layout.rules）の編集。1 つの状態に複数の rule を持て、それぞれが対象（target）と look を持つ。
  // 配列の順が優先順位で、後ろの rule が勝つ（仕様 §3.2）。
  import { KNOWN_ACTIONS, KEY_ROLES, LOOK_LIMITS, LOOK_STATE_INFO } from '$lib/keyboard/types'
  import type { KeyRole, StateName, Target } from '$lib/keyboard/types'
  import { allKeyIds } from '$lib/keyboard/ops'
  import { newTarget, readHoldMs, rulesFor, targetKind } from '$lib/keyboard/look-ops'
  import type { InheritContext, TargetKind } from '$lib/keyboard/look-ops'
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import LookPropsEditor from './LookPropsEditor.svelte'
  import NumberField from './NumberField.svelte'

  interface Props {
    store: EditorStore
    when: StateName
    ctx?: InheritContext
  }
  let { store, when, ctx }: Props = $props()

  const KIND_LABELS: Record<TargetKind, string> = {
    all: '全キー',
    others: '他のキー（移動中の起点以外）',
    keys: '指定キー',
    roles: '役割',
    actions: '操作 (action)'
  }
  const ROLE_LABELS: Record<KeyRole, string> = { normal: 'normal（通常）', function: 'function（機能キー）', accent: 'accent（強調）' }

  const rules = $derived(rulesFor(store.layout, when))
  const keyIds = $derived(allKeyIds(store.layout))
  const actionNames = $derived([
    ...new Set([
      ...KNOWN_ACTIONS,
      ...store.layout.rows.flatMap((r) => r.keys.flatMap((k) => ('action' in k && typeof k.action === 'string' ? [k.action] : [])))
    ])
  ])
  const uid = $props.id()
  let draft = $state<Record<string, string>>({})

  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
  const listOf = (target: unknown, field: 'keys' | 'roles' | 'actions'): string[] =>
    isObj(target) && Array.isArray(target[field]) ? (target[field] as unknown[]).filter((x): x is string => typeof x === 'string') : []
  const kindsFor = (current: TargetKind | undefined): TargetKind[] => {
    const base: TargetKind[] = when === 'spaceDrag' ? ['all', 'others', 'keys', 'roles', 'actions'] : ['all', 'keys', 'roles', 'actions']
    return current && !base.includes(current) ? [...base, current] : base
  }

  function add() {
    const i = store.addRule(when, when === 'spaceDrag' ? 'others' : undefined)
    if (i >= 0) store.notice = null
  }
  function changeKind(index: number, kind: TargetKind) {
    store.setRuleTarget(index, newTarget(kind))
  }
  function setList(index: number, field: 'keys' | 'roles' | 'actions', list: string[]) {
    store.setRuleTarget(index, { [field]: list } as Target, true)
  }
  function addItem(index: number, field: 'keys' | 'actions', current: string[], value: string) {
    const v = value.trim()
    if (v === '' || current.includes(v)) return
    if (field === 'keys' && current.length >= LOOK_LIMITS.targetKeysMax) {
      store.notice = `指定キーは最大 ${LOOK_LIMITS.targetKeysMax} 個までです`
      return
    }
    setList(index, field, [...current, v])
  }
</script>

<section class="rules" aria-label="{LOOK_STATE_INFO[when].label}のとき全体に適用する指定">
  <p class="lead">
    「{LOOK_STATE_INFO[when].label}」のとき、対象のキーに重ねる見た目です。上から順に重ね、下の指定が勝ちます。キー個別の指定は、キーを選んで同じ状態タブで編集します。
  </p>
  {#if rules.length === 0}<p class="hint">この状態の指定はありません。</p>{/if}
  {#each rules as { index, rule }, n (index)}
    {@const kind = targetKind(rule.target)}
    <div class="rule" role="group" aria-label="指定 {n + 1}（rules[{index}]）">
      <div class="rhead">
        <strong>指定 {n + 1}</strong>
        <span class="ctl">
          <button type="button" class="btn small" aria-label="指定 {n + 1} を前へ（優先度を下げる）" disabled={n === 0} onclick={() => store.moveRule(index, rules[n - 1].index)}>↑</button>
          <button type="button" class="btn small" aria-label="指定 {n + 1} を後ろへ（優先度を上げる）" disabled={n === rules.length - 1} onclick={() => store.moveRule(index, rules[n + 1].index)}>↓</button>
          <button type="button" class="btn small danger" aria-label="指定 {n + 1} を削除" onclick={() => store.removeRule(index)}>削除</button>
        </span>
      </div>
      <label class="field">
        <span>対象 (target)</span>
        <select value={kind ?? 'all'} onchange={(e) => changeKind(index, e.currentTarget.value as TargetKind)}>
          {#each kindsFor(kind) as k (k)}<option value={k}>{KIND_LABELS[k]}</option>{/each}
        </select>
      </label>
      {#if kind === undefined}
        <p class="err">target の形が不正なため、この指定は無視されます。対象を選び直してください。</p>
      {:else if kind === 'others' && when !== 'spaceDrag'}
        <p class="hint">「他のキー」は移動中のときだけ有効です（他の状態では全キーと同じ扱い）。</p>
      {:else if kind === 'keys' || kind === 'actions'}
        {@const field = kind}
        {@const list = listOf(rule.target, field)}
        <div class="chips" aria-label={KIND_LABELS[kind]}>
          {#each list as v (v)}
            <span class="chip">{v}<button type="button" aria-label="{v} を外す" onclick={() => setList(index, field, list.filter((x) => x !== v))}>×</button></span>
          {:else}
            <span class="hint">対象がありません（何にも適用されません）</span>
          {/each}
        </div>
        <div class="add">
          <input
            list="{uid}-{index}-{field}"
            aria-label={field === 'keys' ? 'キー id を追加' : 'action 名を追加'}
            placeholder={field === 'keys' ? 'キー id' : 'action 名'}
            spellcheck="false"
            value={draft[`${index}`] ?? ''}
            oninput={(e) => (draft[`${index}`] = e.currentTarget.value)}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                addItem(index, field, list, draft[`${index}`] ?? '')
                draft[`${index}`] = ''
              }
            }}
          />
          <datalist id="{uid}-{index}-{field}">
            {#each field === 'keys' ? keyIds : actionNames as v (v)}<option value={v}></option>{/each}
          </datalist>
          <button
            type="button"
            class="btn small"
            onclick={() => {
              addItem(index, field, list, draft[`${index}`] ?? '')
              draft[`${index}`] = ''
            }}>追加</button
          >
          {#if field === 'keys'}
            <button
              type="button"
              class="btn small"
              disabled={store.selectedIds.length === 0}
              onclick={() => setList(index, 'keys', [...new Set([...list, ...store.selectedIds])].slice(0, LOOK_LIMITS.targetKeysMax))}>選択中のキーを追加</button
            >
          {/if}
        </div>
        {#if field === 'keys'}<p class="hint">{list.length} / {LOOK_LIMITS.targetKeysMax} 個。レイアウトに無い id は無視されます。</p>{/if}
      {:else if kind === 'roles'}
        {@const list = listOf(rule.target, 'roles')}
        <div class="roles">
          {#each KEY_ROLES as r (r)}
            <label class="chk">
              <input
                type="checkbox"
                checked={list.includes(r)}
                onchange={(e) => setList(index, 'roles', e.currentTarget.checked ? [...list, r] : list.filter((x) => x !== r))}
              />
              <span>{ROLE_LABELS[r]}</span>
            </label>
          {/each}
        </div>
      {/if}
      {#if when === 'released'}
        {@const hold = readHoldMs(store.layout, { kind: 'rule', index })}
        <div class="hold">
          <label class="chk">
            <input type="checkbox" checked={hold !== undefined} onchange={(e) => store.setHoldMs([{ kind: 'rule', index }], e.currentTarget.checked ? LOOK_LIMITS.holdMsDefault : undefined)} />
            <span>戻るまでの時間 (holdMs) を指定</span>
          </label>
          {#if hold !== undefined}
            <NumberField
              value={typeof hold === 'number' ? hold : undefined}
              min={LOOK_LIMITS.holdMsMin}
              max={LOOK_LIMITS.holdMsMax}
              step={10}
              unit="ms"
              label="戻るまでの時間"
              onchange={(v) => store.setHoldMs([{ kind: 'rule', index }], v)}
            />
          {:else}
            <span class="hint">既定 {LOOK_LIMITS.holdMsDefault} ms</span>
          {/if}
        </div>
      {/if}
      <LookPropsEditor {store} locs={[{ kind: 'rule', index }]} {ctx} allowContent />
    </div>
  {/each}
  <button type="button" class="btn small" onclick={add}>＋ 対象を追加（指定を足す）</button>
  <p class="hint" aria-live="polite">指定の総数は全状態で最大 {LOOK_LIMITS.rulesMax} 個です（今 {store.layout.rules?.length ?? 0} 個）。</p>
</section>

<style>
  .rules {
    display: grid;
    gap: 8px;
  }
  .lead,
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .err {
    margin: 0;
    color: var(--danger);
    font-size: 0.8125rem;
  }
  .rule {
    display: grid;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--surface-2);
  }
  .rhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .ctl {
    display: flex;
    gap: 4px;
  }
  .field {
    display: grid;
    gap: 2px;
    font-size: 0.875rem;
  }
  .field span {
    color: var(--muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 1px 4px 1px 8px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface);
    font-size: 0.8125rem;
  }
  .chip button {
    border: 0;
    background: none;
    cursor: pointer;
    color: var(--muted);
    padding: 2px 6px;
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .add input {
    flex: 1 1 120px;
    min-height: 28px;
    box-sizing: border-box;
  }
  .roles,
  .hold {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 0.875rem;
  }
</style>
