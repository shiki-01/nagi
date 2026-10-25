// 見た目（look / states / rules / metrics / background / assets）の編集操作。ops.ts と同じく、すべて純粋関数
// （入力を変更せず新しい値を返す。変更が無ければ入力をそのまま返す）。空になった look / states / roles / background /
// metrics / rules / assets は取り除く（JSON を汚さない。docs/keyboard-style-spec.md §8.1）。

import { canAddAsset, dataUriBytes } from './look-assets'
import { resolveLook } from './look-resolve'
import type { LookContext } from './look-resolve'
import { clone, findKey } from './ops'
import { ASSET_ID_PATTERN, BACKGROUND_PROPS, KEY_ROLES, LOOK_CONTENT_PROPS, LOOK_LIMITS, LOOK_PROPS, METRICS_PROPS, TOOLBAR_ROW } from './types'
import type { Key, KeyRole, KeyboardLayout, Metrics, StateName, Target } from './types'

type Obj = Record<string, unknown>
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
const hasOwn = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k)
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)

/** look を書く場所。 */
export type LookLoc =
  | { kind: 'layout' }
  | { kind: 'role'; role: KeyRole }
  | { kind: 'row'; row: number }
  | { kind: 'key'; keyId: string }
  | { kind: 'keyState'; keyId: string; state: StateName }
  | { kind: 'rule'; index: number }
  | { kind: 'background' }

/** その場所に書けるプロパティか（label / icon は states / rules の中だけ。background は 5 つだけ）。 */
export function allowsProp(loc: LookLoc, prop: string): boolean {
  if (loc.kind === 'background') return (BACKGROUND_PROPS as readonly string[]).includes(prop)
  if (!hasOwn(LOOK_PROPS, prop)) return false
  if ((LOOK_CONTENT_PROPS as readonly string[]).includes(prop)) return loc.kind === 'keyState' || loc.kind === 'rule'
  return true
}

/** 場所の look オブジェクト。create=false なら入力を変更しない。 */
function slotOf(l: KeyboardLayout, loc: LookLoc, create: boolean): Obj | undefined {
  const root = l as unknown as Obj
  const ensure = (parent: Obj, key: string): Obj | undefined => {
    const cur = parent[key]
    if (isObj(cur)) return cur
    if (!create) return undefined
    const fresh: Obj = {}
    parent[key] = fresh
    return fresh
  }
  const keyOf = (id: string): Obj | undefined => {
    const f = findKey(l, id)
    return f ? (f.key as unknown as Obj) : undefined
  }
  switch (loc.kind) {
    case 'layout':
      return ensure(root, 'look')
    case 'background':
      return ensure(root, 'background')
    case 'role': {
      const roles = ensure(root, 'roles')
      return roles && ensure(roles, loc.role)
    }
    case 'row': {
      const row = loc.row === TOOLBAR_ROW ? l.toolbar : l.rows[loc.row]
      return row ? ensure(row as unknown as Obj, 'look') : undefined
    }
    case 'key': {
      const key = keyOf(loc.keyId)
      return key && ensure(key, 'look')
    }
    case 'keyState': {
      const key = keyOf(loc.keyId)
      const states = key && ensure(key, 'states')
      return states && ensure(states, loc.state)
    }
    case 'rule': {
      const rules = root.rules
      const rule = Array.isArray(rules) ? rules[loc.index] : undefined
      return isObj(rule) ? ensure(rule, 'look') : undefined
    }
  }
}

/** 空になった入れ物を取り除く（loc の経路のみ）。 */
function prune(l: KeyboardLayout, loc: LookLoc) {
  const root = l as unknown as Obj
  const drop = (parent: unknown, key: string) => {
    if (!isObj(parent)) return
    const v = parent[key]
    if (isObj(v) && Object.keys(v).length === 0) delete parent[key]
  }
  switch (loc.kind) {
    case 'layout':
      return drop(root, 'look')
    case 'background':
      return drop(root, 'background')
    case 'role':
      drop(root.roles, loc.role)
      return drop(root, 'roles')
    case 'row':
      return drop(loc.row === TOOLBAR_ROW ? l.toolbar : l.rows[loc.row], 'look')
    case 'key': {
      const f = findKey(l, loc.keyId)
      return drop(f?.key, 'look')
    }
    case 'keyState': {
      const f = findKey(l, loc.keyId)
      drop((f?.key as unknown as Obj | undefined)?.states, loc.state)
      return drop(f?.key, 'states')
    }
    case 'rule':
      return
  }
}

/** 場所の look（無ければ空）。 */
export function readLook(layout: KeyboardLayout, loc: LookLoc): Readonly<Obj> {
  return slotOf(layout, loc, false) ?? {}
}

/** プロパティを指定する（値は検査しない。範囲外を書いても警告として検証側に出る）。 */
export function setLookProp(layout: KeyboardLayout, loc: LookLoc, prop: string, value: unknown): KeyboardLayout {
  if (!allowsProp(loc, prop) || value === undefined) return layout
  const cur = readLook(layout, loc)
  if (hasOwn(cur, prop) && same(cur[prop], value)) return layout
  const next = clone(layout)
  const slot = slotOf(next, loc, true)
  if (!slot) return layout
  slot[prop] = value
  return next
}

/** プロパティの指定を消す（継承に戻す）。 */
export function clearLookProp(layout: KeyboardLayout, loc: LookLoc, prop: string): KeyboardLayout {
  if (!hasOwn(readLook(layout, loc), prop)) return layout
  const next = clone(layout)
  const slot = slotOf(next, loc, false)
  if (!slot) return layout
  delete slot[prop]
  prune(next, loc)
  return next
}

/** 場所の look を空にする（rule は look を空にして holdMs も消す。rule 自体は残す）。 */
export function resetLook(layout: KeyboardLayout, loc: LookLoc): KeyboardLayout {
  const hold = loc.kind === 'rule' ? ruleHoldMs(layout, loc.index) : undefined
  if (Object.keys(readLook(layout, loc)).length === 0 && hold === undefined) return layout
  const next = clone(layout)
  const slot = slotOf(next, loc, false)
  if (slot) for (const k of Object.keys(slot)) delete slot[k]
  if (loc.kind === 'rule') {
    const r = (next.rules as unknown as Obj[])[loc.index]
    delete r.holdMs
  }
  prune(next, loc)
  return next
}

/** 複数の場所に同じ操作をまとめて行う。 */
export function mapLocs(layout: KeyboardLayout, locs: readonly LookLoc[], fn: (l: KeyboardLayout, loc: LookLoc) => KeyboardLayout): KeyboardLayout {
  return locs.reduce(fn, layout)
}

// ---- holdMs（released のときだけ有効） -----------------------------------------------------------------------

function ruleHoldMs(layout: KeyboardLayout, index: number): unknown {
  const r = Array.isArray(layout.rules) ? (layout.rules[index] as unknown) : undefined
  return isObj(r) ? r.holdMs : undefined
}

/** released の保持時間の指定（rule は rule.holdMs、キーは states.released.holdMs）。未指定は undefined。 */
export function readHoldMs(layout: KeyboardLayout, loc: LookLoc): unknown {
  if (loc.kind === 'rule') return ruleHoldMs(layout, loc.index)
  if (loc.kind === 'keyState') return readLook(layout, loc).holdMs
  return undefined
}

/** holdMs を指定する（undefined で消す）。rule と keyState 以外は何もしない。 */
export function setHoldMs(layout: KeyboardLayout, loc: LookLoc, ms: number | undefined): KeyboardLayout {
  if (loc.kind !== 'rule' && loc.kind !== 'keyState') return layout
  if (same(readHoldMs(layout, loc), ms)) return layout
  const next = clone(layout)
  if (loc.kind === 'rule') {
    const r = Array.isArray(next.rules) ? (next.rules[loc.index] as unknown) : undefined
    if (!isObj(r)) return layout
    if (ms === undefined) delete r.holdMs
    else r.holdMs = ms
    return next
  }
  const slot = slotOf(next, loc, ms !== undefined)
  if (!slot) return layout
  if (ms === undefined) {
    delete slot.holdMs
    prune(next, loc)
  } else slot.holdMs = ms
  return next
}

// ---- キー・全体のリセット ------------------------------------------------------------------------------------

/** キーの見た目（look と states）をすべて消す。 */
export function resetKeyLooks(layout: KeyboardLayout, keyIds: readonly string[]): KeyboardLayout {
  const targets = keyIds.filter((id) => {
    const k = findKey(layout, id)?.key as unknown as Obj | undefined
    return k && (hasOwn(k, 'look') || hasOwn(k, 'states'))
  })
  if (targets.length === 0) return layout
  const next = clone(layout)
  for (const id of targets) {
    const k = findKey(next, id)?.key as unknown as Obj
    delete k.look
    delete k.states
  }
  return next
}

/** 見た目に関する指定（look / roles / rules / metrics / background / assets / 行とキーの look・states）をすべて消す。 */
export function resetAllLooks(layout: KeyboardLayout): KeyboardLayout {
  if (!hasAnyLook(layout)) return layout
  const next = clone(layout) as unknown as Obj
  for (const k of ['look', 'roles', 'rules', 'metrics', 'background', 'assets']) delete next[k]
  for (const row of (next.rows as Obj[]) ?? []) {
    delete row.look
    for (const key of (row.keys as Obj[]) ?? []) {
      delete key.look
      delete key.states
    }
  }
  const tb = next.toolbar
  if (isObj(tb)) {
    delete tb.look
    for (const key of (tb.keys as Obj[]) ?? []) {
      delete key.look
      delete key.states
    }
  }
  return next as unknown as KeyboardLayout
}

export function hasAnyLook(layout: KeyboardLayout): boolean {
  const l = layout as unknown as Obj
  if (['look', 'roles', 'rules', 'metrics', 'background', 'assets'].some((k) => hasOwn(l, k))) return true
  const rows: { look?: unknown; keys: Key[] }[] = layout.toolbar ? [...layout.rows, layout.toolbar] : layout.rows
  return rows.some((r) => hasOwn(r, 'look') || r.keys.some((k) => hasOwn(k, 'look') || hasOwn(k, 'states')))
}

/** 状態のリセット: 全体なら rules の当該 when を全部、キーなら states[state]。 */
export function resetStateOfKeys(layout: KeyboardLayout, keyIds: readonly string[], state: StateName): KeyboardLayout {
  return keyIds.reduce((l, keyId) => resetLook(l, { kind: 'keyState', keyId, state }), layout)
}

// ---- rules ---------------------------------------------------------------------------------------------------

export type TargetKind = 'all' | 'others' | 'keys' | 'roles' | 'actions'
export const TARGET_KINDS: readonly TargetKind[] = ['all', 'others', 'keys', 'roles', 'actions']

export function targetKind(target: unknown): TargetKind | undefined {
  if (target === undefined || target === 'all') return 'all'
  if (target === 'others') return 'others'
  if (isObj(target)) {
    if (Array.isArray(target.keys)) return 'keys'
    if (Array.isArray(target.roles)) return 'roles'
    if (Array.isArray(target.actions)) return 'actions'
  }
  return undefined
}

/** target の種類を切り替えた新しい target（内容は空から）。all は省略（undefined）で表す。 */
export function newTarget(kind: TargetKind): Target | undefined {
  switch (kind) {
    case 'all':
      return undefined
    case 'others':
      return 'others'
    case 'keys':
      return { keys: [] }
    case 'roles':
      return { roles: [] }
    case 'actions':
      return { actions: [] }
  }
}

/** 当該 when の rule（配列内の位置つき）。 */
export function rulesFor(layout: KeyboardLayout, when: StateName): { index: number; rule: Obj }[] {
  const rules: unknown = layout.rules
  if (!Array.isArray(rules)) return []
  return rules.flatMap((r, index) => (isObj(r) && r.when === when ? [{ index, rule: r }] : []))
}

export interface RuleResult {
  layout: KeyboardLayout
  /** 追加後の rule の位置。 */
  index: number
  /** 上限などで実行できなかった理由。 */
  error?: string
}

/** rule を末尾に足す（後ろの rule が勝つ）。上限は rulesMax。 */
export function addRule(layout: KeyboardLayout, when: StateName, target?: Target): RuleResult {
  const rules = Array.isArray(layout.rules) ? layout.rules : []
  if (rules.length >= LOOK_LIMITS.rulesMax)
    return { layout, index: -1, error: `状態の指定（rules）は最大 ${LOOK_LIMITS.rulesMax} 個までです` }
  const next = clone(layout)
  const rule: Obj = { when }
  if (target !== undefined && target !== 'all') rule.target = target
  rule.look = {}
  next.rules = [...(Array.isArray(next.rules) ? next.rules : []), rule as unknown as NonNullable<KeyboardLayout['rules']>[number]]
  return { layout: next, index: next.rules.length - 1 }
}

/** rule の target を差し替える（undefined または 'all' は省略）。 */
export function setRuleTarget(layout: KeyboardLayout, index: number, target: Target | undefined): KeyboardLayout {
  const rules: unknown = layout.rules
  const cur = Array.isArray(rules) ? rules[index] : undefined
  if (!isObj(cur)) return layout
  const want = target === 'all' ? undefined : target
  if (same(cur.target, want)) return layout
  const next = clone(layout)
  const r = (next.rules as unknown as Obj[])[index]
  if (want === undefined) delete r.target
  else r.target = want
  return next
}

export function removeRule(layout: KeyboardLayout, index: number): KeyboardLayout {
  if (!Array.isArray(layout.rules) || index < 0 || index >= layout.rules.length) return layout
  const next = clone(layout)
  next.rules!.splice(index, 1)
  if (next.rules!.length === 0) delete next.rules
  return next
}

/** rule を並べ替える（配列の順が優先順位。後ろが勝つ）。 */
export function moveRule(layout: KeyboardLayout, from: number, to: number): KeyboardLayout {
  const rules = layout.rules
  if (!Array.isArray(rules) || from === to || from < 0 || from >= rules.length || to < 0 || to >= rules.length) return layout
  const next = clone(layout)
  const [r] = next.rules!.splice(from, 1)
  next.rules!.splice(to, 0, r)
  return next
}

/** 当該 when の rule をすべて消す。 */
export function removeRulesFor(layout: KeyboardLayout, when: StateName): KeyboardLayout {
  if (rulesFor(layout, when).length === 0) return layout
  const next = clone(layout)
  next.rules = next.rules!.filter((r) => (r as unknown as Obj).when !== when)
  if (next.rules.length === 0) delete next.rules
  return next
}

// ---- metrics -------------------------------------------------------------------------------------------------

export function setMetric(layout: KeyboardLayout, prop: keyof Metrics, value: number | undefined): KeyboardLayout {
  if (!hasOwn(METRICS_PROPS, prop)) return layout
  const cur = isObj(layout.metrics) ? layout.metrics : undefined
  if (same(cur?.[prop], value)) return layout
  const m: Obj = { ...cur }
  if (value === undefined) delete m[prop]
  else m[prop] = value
  const next = clone(layout)
  if (Object.keys(m).length === 0) delete next.metrics
  else next.metrics = m as Metrics
  return next
}

// ---- assets --------------------------------------------------------------------------------------------------

/** すべての look 置き場（layout.look・roles・行・キーの look と states・rules・background）を巡る。 */
function forEachSlot(l: KeyboardLayout, fn: (slot: Obj) => void) {
  const root = l as unknown as Obj
  const visit = (v: unknown) => {
    if (isObj(v)) fn(v)
  }
  visit(root.look)
  visit(root.background)
  if (isObj(root.roles)) for (const role of KEY_ROLES) visit(root.roles[role])
  if (Array.isArray(root.rules)) for (const r of root.rules) if (isObj(r)) visit(r.look)
  for (const row of l.toolbar ? [...l.rows, l.toolbar] : l.rows) {
    visit((row as unknown as Obj).look)
    for (const key of row.keys) {
      const k = key as unknown as Obj
      visit(k.look)
      if (isObj(k.states)) for (const s of Object.values(k.states)) visit(s)
    }
  }
}

/** 画像 id を参照している指定（fillImage）の数。 */
export function assetRefCount(layout: KeyboardLayout, id: string): number {
  let n = 0
  forEachSlot(layout, (slot) => {
    if (slot.fillImage === id) n++
  })
  return n
}

export interface AssetResult {
  layout: KeyboardLayout
  /** 追加できなかった理由。 */
  error?: string
}

/** 画像を追加する。id の形式・重複、data URI の形式（PNG / JPEG の base64）、個数・1 枚・合計の上限を検査する。 */
export function addAsset(layout: KeyboardLayout, id: string, dataUri: string): AssetResult {
  if (!ASSET_ID_PATTERN.test(id)) return { layout, error: '画像 id は英小文字・数字・- _ で、32 文字以内にしてください' }
  const assets = isObj(layout.assets) ? (layout.assets as Record<string, unknown>) : {}
  if (hasOwn(assets, id)) return { layout, error: `画像 id「${id}」は使われています` }
  const bytes = dataUriBytes(dataUri)
  if (bytes === undefined) return { layout, error: '画像は PNG / JPEG の base64 data URI にしてください' }
  const reason = canAddAsset(assets, bytes)
  if (reason) return { layout, error: reason }
  const next = clone(layout)
  next.assets = { ...(isObj(next.assets) ? next.assets : {}), [id]: dataUri }
  return { layout: next }
}

/** 画像を削除し、それを参照している fillImage の指定も外す。 */
export function removeAsset(layout: KeyboardLayout, id: string): KeyboardLayout {
  if (!isObj(layout.assets) || !hasOwn(layout.assets, id)) return layout
  const next = clone(layout)
  delete next.assets![id]
  if (Object.keys(next.assets!).length === 0) delete next.assets
  forEachSlot(next, (slot) => {
    if (slot.fillImage === id) delete slot.fillImage
  })
  pruneEmpty(next)
  return next
}

/** 空になった look / states / roles / background を（全体で）取り除く。 */
export function pruneEmpty(l: KeyboardLayout) {
  const root = l as unknown as Obj
  const drop = (parent: unknown, key: string) => {
    if (isObj(parent) && isObj(parent[key]) && Object.keys(parent[key] as Obj).length === 0) delete parent[key]
  }
  drop(root, 'look')
  drop(root, 'background')
  if (isObj(root.roles)) {
    for (const role of KEY_ROLES) drop(root.roles, role)
    drop(root, 'roles')
  }
  for (const row of l.toolbar ? [...l.rows, l.toolbar] : l.rows) {
    drop(row, 'look')
    for (const key of row.keys) {
      const k = key as unknown as Obj
      drop(k, 'look')
      if (isObj(k.states)) {
        for (const s of Object.keys(k.states)) drop(k.states, s)
        drop(k, 'states')
      }
    }
  }
}

// ---- 継承される値（エディタの表示用） ------------------------------------------------------------------------

export interface InheritContext {
  rowIndex: number
  key: Key
  states: readonly StateName[]
  origin?: string
  scheme?: LookContext['scheme']
}

/**
 * loc の prop の指定が無かったとしたときの、代表キーの解決結果（継承される値）。何も指定されていなければ undefined（既定）。
 * 色は外観で解決済みの `#RRGGBBAA` か `@トークン`。
 */
export function inheritedValue(layout: KeyboardLayout, loc: LookLoc, prop: string, ctx: InheritContext): string | number | undefined {
  if (loc.kind === 'background') return undefined
  // resolveLook は assets を見ないので、複製のコスト（最大 1MB の data URI）を避けて外す
  const cleared = clearLookProp({ ...layout, assets: undefined }, loc, prop)
  // キー自身の指定を外した後のキー（ctx.key は外す前のもの）で解決する
  const key = findKey(cleared, ctx.key.id)?.key ?? ctx.key
  return resolveLook(cleared, ctx.rowIndex, key, { states: ctx.states, origin: ctx.origin, scheme: ctx.scheme })[prop]
}

/** 「指定する」を入れたときの初期値（継承される値が無いとき）。現行の見た目に近い値。 */
export const LOOK_INITIAL: Readonly<Record<string, number | string>> = {
  fontSize: 17,
  fontWeight: 'regular',
  fontDesign: 'system',
  color: '@label',
  fill: '@key',
  fillOpacity: 1,
  fillImage: 'none',
  fillImageFit: 'fill',
  fillImageOpacity: 1,
  cornerRadius: 6,
  borderWidth: 1,
  borderColor: '@separator',
  shadowColor: '#000000',
  shadowRadius: 4,
  shadowX: 0,
  shadowY: 1,
  shadowOpacity: 0.3,
  opacity: 1,
  scale: 1,
  offsetX: 0,
  offsetY: 0,
  inset: 0,
  padding: 0,
  animationMs: 120
}
