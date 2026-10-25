// 見た目の解決（docs/keyboard-style-spec.md §3〜§4）。純粋関数。
// Swift の LookResolver（frontend-ios-spike/Shared/KeyLookResolver.swift）と同じ規則の TypeScript 実装。
// 二つは config/keyboard/test-looks/*.json の同じテストベクタで同じ結果を出すこと（変えるときは両方とベクタを揃える）。
// エディタが「継承される値」を出すのと、プレビューの描画に使う。

import { parseColor, resolveColor } from './look-color'
import type { ParsedColor, Scheme } from './look-color'
import {
  ICONIFY_ICON_PATTERN,
  KEY_ROLES,
  LOOK_CONTENT_PROPS,
  LOOK_LIMITS,
  LOOK_PROPS,
  LOOK_STATES,
  SF_ICON_PATTERN,
  STATE_PRECEDENCE,
  ASSET_ID_PATTERN,
  TOOLBAR_DEFAULT_LOOK,
  TOOLBAR_ROW
} from './types'
import type { Key, KeyRole, KeyboardLayout, StateName } from './types'

const hasOwn = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k)
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const len = (s: string) => [...s].length

/** 解決の文脈（今の状態）。 */
export interface LookContext {
  /** 有効な状態。局所の状態（pressed など）は、そのキー自身について有効なものを渡す。 */
  states?: Iterable<StateName>
  /** spaceDrag の起点キーの id（target: "others" の判定に使う）。 */
  origin?: string
  scheme?: Scheme
}

/**
 * 解決結果。指定された（または状態の既定で決まった）プロパティだけを持ち、無いものは「既定のまま」。
 * 色は外観で解決済みの `#RRGGBBAA` か `@トークン`。fillImage が無いときは画像なし（"none" は取り除く）。
 */
export type ResolvedLook = Record<string, number | string>

/** 検査済みの値。色は解析済み。 */
type SanitizedValue = number | string | ParsedColor
export type SanitizedLook = Record<string, SanitizedValue>

/** icon の書式の誤り（日本語の理由）。`sf:` / `iconify:` で始まらない文字列は従来どおり SF Symbols 名として通す。 */
export function iconFormatError(v: string): string | undefined {
  if (v.startsWith('iconify:') && !ICONIFY_ICON_PATTERN.test(v))
    return 'iconify: の書式は iconify:<prefix>:<name>（小文字英数字とハイフン。例 iconify:mdi:home）にしてください'
  if (v.startsWith('sf:') && !SF_ICON_PATTERN.test(v)) return 'sf: の後ろは SF Symbols 名（英数字と . _ -）にしてください'
  return undefined
}

export type CheckResult = { ok: true; value: SanitizedValue } | { ok: false; reason: string }

/** look の 1 プロパティの値を検査する（クランプはしない）。name は LOOK_PROPS にあるもの。 */
export function checkLookValue(name: string, value: unknown): CheckResult {
  const spec = LOOK_PROPS[name]
  switch (spec.type) {
    case 'number':
      if (typeof value === 'number' && Number.isFinite(value) && value >= spec.min && value <= spec.max) return { ok: true, value }
      return { ok: false, reason: `${spec.min}〜${spec.max} の数で指定してください` }
    case 'enum':
      if (typeof value === 'string' && spec.values.includes(value)) return { ok: true, value }
      return { ok: false, reason: `${spec.values.join(' / ')} のいずれかにしてください` }
    case 'color': {
      const c = parseColor(value)
      if (c !== undefined) return { ok: true, value: c }
      return { ok: false, reason: '色は #RGB / #RRGGBB / #RRGGBBAA / @トークン / { "light": …, "dark": … } で指定してください' }
    }
    case 'imageRef':
      if (typeof value === 'string' && (value === 'none' || ASSET_ID_PATTERN.test(value))) return { ok: true, value }
      return { ok: false, reason: '画像 id（assets のキー）か "none" で指定してください' }
    case 'label':
      if (typeof value === 'string' && len(value) >= 1 && len(value) <= LOOK_LIMITS.labelMax) return { ok: true, value }
      return { ok: false, reason: `1〜${LOOK_LIMITS.labelMax} 文字の文字列にしてください` }
    case 'icon': {
      if (typeof value !== 'string' || len(value) < 1 || len(value) > LOOK_LIMITS.iconMax)
        return { ok: false, reason: `1〜${LOOK_LIMITS.iconMax} 文字の文字列にしてください` }
      const err = iconFormatError(value)
      return err ? { ok: false, reason: err } : { ok: true, value }
    }
  }
}

/** 範囲外・不正なプロパティを取り除く。allowsContent が false なら label / icon も取り除く。 */
export function sanitizeLook(raw: unknown, allowsContent: boolean): SanitizedLook {
  const out: SanitizedLook = {}
  if (!isObj(raw)) return out
  for (const [name, value] of Object.entries(raw)) {
    if (!hasOwn(LOOK_PROPS, name)) continue
    if (!allowsContent && (LOOK_CONTENT_PROPS as readonly string[]).includes(name)) continue
    const r = checkLookValue(name, value)
    if (r.ok) out[name] = r.value
  }
  return out
}

type ParsedTarget =
  | { kind: 'all' | 'others' }
  | { kind: 'keys' | 'actions'; list: string[] }
  | { kind: 'roles'; list: KeyRole[] }

/** rules[].target を解析する。省略は all。形が不正なら undefined（その rule を無視する）。 */
export function parseTarget(rule: Record<string, unknown>): ParsedTarget | undefined {
  if (!hasOwn(rule, 'target')) return { kind: 'all' }
  const v = rule.target
  if (typeof v === 'string') return v === 'all' || v === 'others' ? { kind: v } : undefined
  if (!isObj(v)) return undefined
  const names = Object.keys(v)
  if (names.length !== 1) return undefined
  const list = v[names[0]]
  if (!Array.isArray(list) || list.some((x) => typeof x !== 'string')) return undefined
  const strings = list as string[]
  switch (names[0]) {
    case 'keys':
      return { kind: 'keys', list: strings }
    case 'roles':
      return { kind: 'roles', list: strings.filter((s): s is KeyRole => (KEY_ROLES as readonly string[]).includes(s)) }
    case 'actions':
      return { kind: 'actions', list: strings }
    default:
      return undefined
  }
}

function roleOf(key: Key): KeyRole {
  return (KEY_ROLES as readonly string[]).includes(key.role as string) ? (key.role as KeyRole) : 'normal'
}

/** target がキーに当てはまるか。spacer は含めない。origin は spaceDrag の起点キーの id。 */
function matches(target: ParsedTarget, key: Key, when: StateName, origin: string | undefined): boolean {
  if (key.kind === 'spacer') return false
  switch (target.kind) {
    case 'all':
      return true
    case 'others':
      return when !== 'spaceDrag' || origin === undefined || key.id !== origin
    case 'keys':
      return target.list.includes(key.id)
    case 'roles':
      return target.list.includes(roleOf(key))
    case 'actions':
      return 'action' in key && typeof key.action === 'string' && target.list.includes(key.action)
  }
}

/** 読める rule（object で when が既知のもの）。target の形は parseTarget で見る。 */
function rulesOf(layout: KeyboardLayout): Record<string, unknown>[] {
  const rules: unknown = layout.rules
  if (!Array.isArray(rules)) return []
  return rules.filter((r): r is Record<string, unknown> => isObj(r) && (LOOK_STATES as readonly unknown[]).includes(r.when))
}

function stateLookOf(key: Key, state: StateName): unknown {
  const states: unknown = key.states
  return isObj(states) ? states[state] : undefined
}

/**
 * キー 1 つの見た目を解決する。重ねる順（低 → 高。後が先を上書き）:
 * layout.look → layout.roles[role] → 行の look → キーの look →
 * 有効な状態を優先順位（STATE_PRECEDENCE）の低い順に、それぞれ
 * 〔組み込みの既定（pressed の fillOpacity 0.6）→ 一致する rules（配列順）→ キーの states[状態]〕。
 * 指定が無いプロパティは結果に含まれない（既定のまま）。spacer は何も持たない。
 * rowIndex が TOOLBAR_ROW（-1）のときは layout.toolbar を行として扱い（行の look = toolbar.look）、最下位に既定（TOOLBAR_DEFAULT_LOOK）を重ねる。
 */
export function resolveLook(layout: KeyboardLayout, rowIndex: number, key: Key, ctx: LookContext = {}): ResolvedLook {
  if (key.kind === 'spacer') return {}
  const scheme = ctx.scheme ?? 'light'
  const states = new Set<StateName>(ctx.states ?? [])
  const out: ResolvedLook = {}
  const apply = (raw: unknown, allowsContent: boolean) => {
    for (const [name, value] of Object.entries(sanitizeLook(raw, allowsContent))) {
      if (LOOK_PROPS[name].type === 'color') {
        // 片方の外観だけ指定した色は、書いていない側では未指定扱い（下位の値を残す）
        const solid = resolveColor(value as ParsedColor, scheme)
        if (solid !== undefined) out[name] = solid
      } else out[name] = value as number | string
    }
  }

  if (rowIndex === TOOLBAR_ROW) apply(TOOLBAR_DEFAULT_LOOK, false)
  apply(layout.look, false)
  const roles: unknown = layout.roles
  if (isObj(roles)) apply(roles[roleOf(key)], false)
  const row: unknown = rowIndex === TOOLBAR_ROW ? layout.toolbar : layout.rows?.[rowIndex]
  if (isObj(row)) apply(row.look, false)
  apply(key.look, false)
  const rules = rulesOf(layout)
  for (const state of STATE_PRECEDENCE) {
    if (!states.has(state)) continue
    if (state === 'pressed') apply({ fillOpacity: LOOK_LIMITS.pressedFillOpacity }, false)
    for (const rule of rules) {
      if (rule.when !== state) continue
      const target = parseTarget(rule)
      if (target && matches(target, key, state, ctx.origin)) apply(rule.look, true)
    }
    apply(stateLookOf(key, state), true)
  }
  // "none" は下位で指定された画像を外す指定。結果には画像なしとして残さない
  if (out.fillImage === 'none') delete out.fillImage
  return out
}

const validHold = (v: unknown): v is number =>
  typeof v === 'number' && Number.isFinite(v) && v >= LOOK_LIMITS.holdMsMin && v <= LOOK_LIMITS.holdMsMax

/**
 * released の保持時間（ms。0〜2000、既定 200）。一致する rules（配列順）→ キーの states.released の順で、
 * 範囲内の holdMs を指定した最後のものが勝つ。
 */
export function releasedHoldMs(layout: KeyboardLayout, key: Key): number {
  let hold: number = LOOK_LIMITS.holdMsDefault
  for (const rule of rulesOf(layout)) {
    if (rule.when !== 'released') continue
    const target = parseTarget(rule)
    const ms = rule.holdMs
    if (target && matches(target, key, 'released', undefined) && validHold(ms)) hold = ms
  }
  const own = stateLookOf(key, 'released')
  const ownMs = isObj(own) ? own.holdMs : undefined
  if (validHold(ownMs)) hold = ownMs
  return hold
}

/** 状態が変わったときの補間の長さ（ms）。前後の解決結果の animationMs の大きいほう（§3.4）。視差効果を減らすときは 0。 */
export function transitionMs(from: ResolvedLook, to: ResolvedLook, reduceMotion: boolean): number {
  if (reduceMotion) return 0
  const ms = (l: ResolvedLook) => (typeof l.animationMs === 'number' ? l.animationMs : 0)
  return Math.max(ms(from), ms(to))
}

/** 表示する文字・アイコンの上書き（§2.3）。label だけのときは hidesKeyIcon（キー本来のアイコンを出さない）。 */
export interface LookContent {
  label: string | null
  icon: string | null
  hidesKeyIcon: boolean
}

export function lookContent(look: ResolvedLook): LookContent {
  const label = typeof look.label === 'string' ? look.label : null
  const icon = typeof look.icon === 'string' ? look.icon : null
  return { label, icon, hidesKeyIcon: label !== null && icon === null }
}
