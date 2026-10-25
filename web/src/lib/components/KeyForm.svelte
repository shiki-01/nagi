<script lang="ts">
  import type { EditorStore } from '$lib/keyboard/store.svelte'
  import { findKey, keysAt } from '$lib/keyboard/ops'
  import { BUILTIN_LAYOUT_IDS, ICONIFY_ICON_PATTERN, FLICK_DIRECTIONS, KEY_KINDS, KEY_ROLES, KNOWN_ACTIONS, LIMITS, POPUP_KINDS, TOOLBAR_ROW } from '$lib/keyboard/types'
  import type { FlickDirection, Key, KeyKind, PopupKey, PopupKind, Step } from '$lib/keyboard/types'
  import StepsEditor from './StepsEditor.svelte'

  let { store }: { store: EditorStore } = $props()

  const KIND_LABELS: Record<KeyKind, string> = {
    text: 'text: 文字入力',
    flick: 'flick: フリック',
    toggle: 'toggle: トグル（ガラケー式）',
    action: 'action: 操作',
    shortcut: 'shortcut: ショートカット',
    layout: 'layout: レイアウト切替',
    spacer: 'spacer: 空白'
  }
  const ROLE_LABELS = { normal: 'normal（通常）', function: 'function（機能キー）', accent: 'accent（強調）' }
  const DIR_LABELS: Record<FlickDirection, string> = { up: '上', down: '下', left: '左', right: '右' }
  const POPUP_LABELS: Record<PopupKind, string> = { text: '文字', action: '操作', shortcut: 'ショートカット' }

  const key = $derived(store.selectedKey)
  const pos = $derived(key ? findKey(store.layout, key.id) : undefined)
  const inToolbar = $derived(pos?.row === TOOLBAR_ROW)
  const rowName = (row: number) => (row === TOOLBAR_ROW ? 'ツールバー' : `${row + 1} 行目`)
  /** 隣の行（上: -1 の向き）へ移す先。ツールバーは rows の上にある。無ければ undefined。 */
  const neighbor = (delta: -1 | 1) => (pos && keysAt(store.layout, pos.row + delta) ? pos.row + delta : undefined)
  function moveToRow(delta: -1 | 1) {
    const to = neighbor(delta)
    if (!key || to === undefined) return
    store.moveKeys([key.id], { row: to, index: keysAt(store.layout, to)?.length ?? 0 })
  }
  const layoutTargets = $derived([...new Set([...BUILTIN_LAYOUT_IDS, ...store.state.layouts.map((l) => l.id)])])

  // iconify: のアイコンはブラウザから Iconify API を直接読んでプレビューする（書き出す JSON には ID だけ）
  const iconify = $derived.by(() => {
    const m = key?.icon ? ICONIFY_ICON_PATTERN.exec(key.icon) : null
    return m ? { prefix: m[1], name: m[2], url: `https://api.iconify.design/${m[1]}/${m[2]}.svg` } : null
  })
  let iconFailed = $state<string | null>(null)

  /** 現在のキーを書き換える。 */
  function update(fn: (k: Key) => Key, coalesce?: string) {
    if (!key) return
    store.updateKey(key.id, fn(key), coalesce)
  }
  /** 任意フィールドの更新（空文字は削除）。 */
  function setOpt(field: 'label' | 'icon', value: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (value === '') delete next[field]
      else next[field] = value
      return next as unknown as Key
    }, field)
  }
  function setField<K extends string>(field: K, value: unknown, coalesce = field as string) {
    update((k) => ({ ...k, [field]: value }) as unknown as Key, coalesce)
  }
  function setWidth(v: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      const n = Number(v)
      if (v === '' || !Number.isFinite(n)) delete next.width
      else next.width = n
      return next as unknown as Key
    }, 'width')
  }
  function setRole(v: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (v === 'normal' || v === '') delete next.role
      else next.role = v
      return next as unknown as Key
    })
  }
  function setFlick(dir: 'center' | FlickDirection, v: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (v === '' && dir !== 'center') delete next[dir]
      else next[dir] = { text: v }
      return next as unknown as Key
    }, `flick-${dir}`)
  }
  function setKana(v: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (v === '') delete next.kana
      else next.kana = v
      return next as unknown as Key
    }, 'kana')
  }
  function setShift(v: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (v === '') delete next.shift
      else next.shift = v
      return next as unknown as Key
    }, 'shift')
  }
  function setVisible(field: string, list: string) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      const items = list.split(',').map((s) => s.trim()).filter(Boolean)
      if (field === '' && items.length === 0) delete next.visible
      else next.visible = { field, in: items }
      return next as unknown as Key
    }, 'visible')
  }
  function setLongPress(list: PopupKey[]) {
    update((k) => {
      const next = { ...k } as Record<string, unknown>
      if (list.length === 0) delete next.longPress
      else next.longPress = list
      return next as unknown as Key
    })
  }
  function setPopup(i: number, p: PopupKey, coalesce?: string) {
    update(
      (k) => ({ ...k, longPress: (k.longPress ?? []).map((x, j) => (j === i ? p : x)) }) as Key,
      coalesce ? `lp${i}:${coalesce}` : undefined
    )
  }
  function changePopupKind(i: number, p: PopupKey, kind: PopupKind) {
    const label = p.label
    setPopup(i, kind === 'text' ? { label, kind, text: label } : kind === 'action' ? { label, kind, action: 'space' } : { label, kind, steps: [{ text: label }] })
  }
  function addPopup() {
    setLongPress([...(key?.longPress ?? []), { label: '@', kind: 'text', text: '@' }])
  }
  function setSteps(steps: Step[], coalesce?: string) {
    update((k) => ({ ...k, steps }) as Key, coalesce)
  }
  function setCycle(cycle: string[], coalesce?: string) {
    update((k) => ({ ...k, cycle }) as Key, coalesce)
  }
  const idCounter = `kf${Math.random().toString(36).slice(2, 7)}`
</script>

{#if key && pos}
  <section class="form" aria-labelledby="kf-title">
    <div class="head">
      <h3 id="kf-title">キーの設定 <small>（{rowName(pos.row)} / {pos.index + 1} 番目）</small></h3>
      <div class="ops">
        <button type="button" class="btn small" disabled={pos.index === 0} onclick={() => store.shiftKey(key.id, -1)}>← 左へ</button>
        <button
          type="button"
          class="btn small"
          disabled={pos.index === (keysAt(store.layout, pos.row)?.length ?? 0) - 1}
          onclick={() => store.shiftKey(key.id, 1)}>右へ →</button
        >
        <button type="button" class="btn small" disabled={neighbor(-1) === undefined} onclick={() => moveToRow(-1)}>
          {pos.row === 0 && store.layout.toolbar ? '↑ ツールバーへ' : '↑ 前の行へ'}
        </button>
        <button type="button" class="btn small" disabled={neighbor(1) === undefined} onclick={() => moveToRow(1)}>
          {inToolbar ? '↓ 1 行目へ' : '↓ 次の行へ'}
        </button>
        <button type="button" class="btn small" onclick={() => store.duplicateKey(key.id)}>複製</button>
        <button type="button" class="btn small danger" onclick={() => store.removeKey(key.id)}>削除</button>
      </div>
    </div>

    <div class="grid">
      <label class="field">
        <span>種類 (kind)</span>
        <select value={key.kind} onchange={(e) => store.changeKind(key.id, e.currentTarget.value as KeyKind)}>
          {#each KEY_KINDS as k (k)}<option value={k}>{KIND_LABELS[k]}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>id（レイアウト内で一意）</span>
        <input value={key.id} oninput={(e) => setField('id', e.currentTarget.value, 'id')} spellcheck="false" />
      </label>
      <label class="field">
        <span>幅 (width, 行内の比)</span>
        <input type="number" min="0.25" max={LIMITS.widthMax} step="0.25" value={key.width ?? 1} oninput={(e) => setWidth(e.currentTarget.value)} />
      </label>
      {#if key.kind !== 'spacer'}
        <label class="field">
          <span>ラベル (label)</span>
          <input value={key.label ?? ''} maxlength={LIMITS.labelMax} oninput={(e) => setOpt('label', e.currentTarget.value)} />
        </label>
        <label class="field">
          <span>アイコン (icon: sf:SF Symbols 名 / iconify:prefix:name)</span>
          <input value={key.icon ?? ''} maxlength={LIMITS.iconMax} spellcheck="false" placeholder="iconify:mdi:home" oninput={(e) => setOpt('icon', e.currentTarget.value)} />
          {#if iconify}
            <span class="iconprev">
              {#if iconFailed === iconify.url}
                <span>取得できません。アプリでも取得に失敗した場合は label が表示されます</span>
              {:else}
                {#key iconify.url}
                  <img src={iconify.url} alt={key.icon} height="24" onerror={() => (iconFailed = iconify.url)} />
                {/key}
                <span>プレビュー（Iconify）。JSON には ID のみ入ります</span>
              {/if}
            </span>
          {/if}
        </label>
        <label class="field">
          <span>色の役割 (role)</span>
          <select value={key.role ?? 'normal'} onchange={(e) => setRole(e.currentTarget.value)}>
            {#each KEY_ROLES as s (s)}<option value={s}>{ROLE_LABELS[s]}</option>{/each}
          </select>
        </label>
      {/if}
    </div>

    <!-- kind ごとの専用フォーム -->
    {#if key.kind === 'text'}
      <div class="grid">
        <label class="field">
          <span>送る文字 (text)</span>
          <input value={key.text} maxlength={LIMITS.textMax} oninput={(e) => setField('text', e.currentTarget.value)} />
        </label>
        <label class="field">
          <span>シフト時の文字 (shift)</span>
          <input value={key.shift ?? ''} maxlength={LIMITS.textMax} oninput={(e) => setShift(e.currentTarget.value)} />
        </label>
      </div>
      <p class="hint">QWERTY のかなモードでは、この文字はコアのローマ字変換に渡されます。</p>
    {:else if key.kind === 'flick'}
      <fieldset>
        <legend>フリックの文字（4 方向）</legend>
        <div class="grid">
          <label class="field">
            <span>中央（タップ）</span>
            <input value={(key.center?.text ?? key.kana ?? "")} maxlength={LIMITS.textMax} oninput={(e) => setFlick('center', e.currentTarget.value)} />
          </label>
          {#each FLICK_DIRECTIONS as d (d)}
            <label class="field">
              <span>{DIR_LABELS[d]}フリック</span>
              <input value={key[d]?.text ?? ''} maxlength={LIMITS.textMax} oninput={(e) => setFlick(d, e.currentTarget.value)} />
            </label>
          {/each}
          <label class="field">
            <span>12 キー名 (kana, 例: あ)</span>
            <input value={key.kana ?? ''} maxlength={LIMITS.kanaMax} oninput={(e) => setKana(e.currentTarget.value)} />
          </label>
        </div>
      </fieldset>
      <p class="hint">kana を指定すると、トグル・フリック・濁点循環はコアに任せます（上下左右の文字は表示用）。</p>
    {:else if key.kind === 'toggle'}
      <fieldset>
        <legend>押すたびに切り替わる文字 (cycle, 1〜{LIMITS.cycleMax})</legend>
        <ol class="list">
          {#each key.cycle as c, i (i)}
            <li>
              <input
                aria-label="{i + 1} 番目の文字"
                value={c}
                maxlength={LIMITS.textMax}
                oninput={(e) => setCycle(key.cycle.map((x, j) => (j === i ? e.currentTarget.value : x)), `cycle${i}`)}
              />
              <button type="button" class="btn small danger" disabled={key.cycle.length <= 1} aria-label="{i + 1} 番目を削除" onclick={() => setCycle(key.cycle.filter((_, j) => j !== i))}>削除</button>
            </li>
          {/each}
        </ol>
        <button type="button" class="btn small" disabled={key.cycle.length >= LIMITS.cycleMax} onclick={() => setCycle([...key.cycle, ''])}>追加</button>
      </fieldset>
    {:else if key.kind === 'action'}
      <label class="field">
        <span>操作 (action)</span>
        <input list="actions-{idCounter}" value={key.action} spellcheck="false" oninput={(e) => setField('action', e.currentTarget.value)} />
        <datalist id="actions-{idCounter}">
          {#each KNOWN_ACTIONS as a (a)}<option value={a}></option>{/each}
        </datalist>
      </label>
    {:else if key.kind === 'shortcut'}
      <StepsEditor uid={`${idCounter}-sc`} legend="手順 (steps)：上から順に実行" steps={key.steps} onchange={setSteps} />
    {:else if key.kind === 'layout'}
      <label class="field">
        <span>切り替え先 (target)</span>
        <input list="targets-{idCounter}" value={key.target} spellcheck="false" oninput={(e) => setField('target', e.currentTarget.value)} />
        <datalist id="targets-{idCounter}">
          {#each layoutTargets as t (t)}<option value={t}></option>{/each}
        </datalist>
      </label>
    {:else}
      <p class="hint">空白キーは幅だけを持ちます。</p>
    {/if}

    {#if key.kind !== 'spacer'}
      <fieldset>
        <legend>長押しメニュー (longPress)</legend>
        {#if key.longPress?.length}
          <ol class="popups">
            {#each key.longPress as p, i (i)}
              <li>
                <div class="row">
                  <label class="field">
                    <span>表示 {i + 1}</span>
                    <input value={p.label} maxlength={LIMITS.labelMax} oninput={(e) => setPopup(i, { ...p, label: e.currentTarget.value }, 'label')} />
                  </label>
                  <label class="field">
                    <span>種類</span>
                    <select value={p.kind} onchange={(e) => changePopupKind(i, p, e.currentTarget.value as PopupKind)}>
                      {#each POPUP_KINDS as k (k)}<option value={k}>{POPUP_LABELS[k]}</option>{/each}
                    </select>
                  </label>
                  {#if p.kind === 'text'}
                    <label class="field">
                      <span>文字</span>
                      <input value={p.text} maxlength={LIMITS.textMax} oninput={(e) => setPopup(i, { ...p, text: e.currentTarget.value }, 'text')} />
                    </label>
                  {:else if p.kind === 'action'}
                    <label class="field">
                      <span>操作</span>
                      <input list="actions-{idCounter}" value={p.action} spellcheck="false" oninput={(e) => setPopup(i, { ...p, action: e.currentTarget.value }, 'action')} />
                    </label>
                  {/if}
                  <button type="button" class="btn small danger" aria-label="長押し {i + 1} を削除" onclick={() => setLongPress((key.longPress ?? []).filter((_, j) => j !== i))}>削除</button>
                </div>
                {#if p.kind === 'shortcut'}
                  <StepsEditor uid={`${idCounter}-lp${i}`} legend="手順" steps={p.steps} onchange={(steps, c) => setPopup(i, { ...p, steps }, c)} />
                {/if}
              </li>
            {/each}
          </ol>
        {:else}
          <p class="hint">なし。長押しで出す文字や操作を追加できます。</p>
        {/if}
        <button type="button" class="btn small" disabled={(key.longPress?.length ?? 0) >= LIMITS.longPressMax} onclick={addPopup}>長押しの項目を追加</button>
      </fieldset>
    {/if}

    <details>
      <summary>表示条件 (visible)</summary>
      <div class="grid">
        <label class="field">
          <span>field（例: returnKeyType）</span>
          <input value={key.visible?.field ?? ''} oninput={(e) => setVisible(e.currentTarget.value, (key.visible?.in ?? []).join(','))} />
        </label>
        <label class="field">
          <span>in（カンマ区切り。例: search）</span>
          <input value={(key.visible?.in ?? []).join(', ')} oninput={(e) => setVisible(key.visible?.field ?? '', e.currentTarget.value)} />
        </label>
      </div>
      <p class="hint">field を空にして in も空にすると条件を外します。v1 では省略できます。</p>
    </details>
  </section>
{:else}
  <p class="empty">プレビューまたは配置一覧でキーを選ぶと、ここで設定できます。</p>
{/if}

<style>
  .iconprev {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.8em;
    opacity: 0.8;
  }
  .iconprev img {
    height: 24px;
    width: auto;
  }
  .form {
    display: grid;
    gap: 12px;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: space-between;
    align-items: center;
  }
  h3 {
    margin: 0;
    font-size: 1.0625rem;
  }
  h3 small {
    font-weight: 400;
    color: var(--muted);
  }
  .ops {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .grid {
    display: grid;
    gap: 10px;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 180px), 1fr));
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: flex-end;
  }
  .row .field {
    flex: 1 1 120px;
  }
  .field {
    display: grid;
    gap: 2px;
    font-size: 0.875rem;
    min-width: 0;
  }
  .field span {
    color: var(--muted);
  }
  fieldset {
    border: 1px solid var(--line);
    border-radius: 10px;
    margin: 0;
    padding: 8px 10px 10px;
    min-width: 0;
  }
  legend {
    font-size: 0.875rem;
    padding: 0 4px;
  }
  .hint,
  .empty {
    color: var(--muted);
    font-size: 0.875rem;
    margin: 0;
  }
  .list,
  .popups {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
    display: grid;
    gap: 6px;
  }
  .list li {
    display: flex;
    gap: 6px;
  }
  .list input {
    flex: 1;
    min-width: 0;
  }
  .popups li {
    display: grid;
    gap: 6px;
    padding-bottom: 8px;
    border-bottom: 1px dashed var(--line);
  }
  summary {
    cursor: pointer;
    padding: 6px 0;
  }
</style>
