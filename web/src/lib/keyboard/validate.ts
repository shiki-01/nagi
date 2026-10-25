// config/keyboard/layout.schema.json を手書きで検証する（依存を増やさないため）。
// JSON Schema で表せない規則（行内の幅の合計、id の一意性、layout.target の存在）もここで見る。

import { dataUriBytes, imageDimensions } from './look-assets'
import { checkLookValue, iconFormatError, parseTarget } from './look-resolve'
import {
  ACTION_PATTERN,
  ASSET_ID_PATTERN,
  ASSET_LIMITS,
  BACKGROUND_PROPS,
  BUILTIN_LAYOUT_IDS,
  FLICK_DIRECTIONS,
  KEY_ID_PATTERN,
  KEY_KINDS,
  KEY_ROLES,
  LAYOUT_ID_PATTERN,
  LAYOUT_MODES,
  LIMITS,
  LOOK_CONTENT_PROPS,
  LOOK_LIMITS,
  LOOK_PROPS,
  LOOK_STATES,
  METRICS_PROPS,
  POPUP_KINDS,
  SCHEMA_VERSION
} from './types'
import type { KeyboardLayout } from './types'

export type Severity = 'error' | 'warning'

export interface ValidationError {
  /** 例: `rows[0].keys[2].text`。レイアウト全体なら空文字。 */
  path: string
  message: string
  /**
   * `error` は取り込み・書き出しをブロックする構造の破損と資源の上限。`warning` は iOS が無視する見た目の値の問題
   * （書き出しはブロックしない）。省略時は `error`。docs/keyboard-style-spec.md §1.4
   */
  severity?: Severity
}

/** error だけを取り出す（省略は error）。 */
export const errorsOf = (issues: readonly ValidationError[]): ValidationError[] => issues.filter((e) => e.severity !== 'warning')
/** warning だけを取り出す。 */
export const warningsOf = (issues: readonly ValidationError[]): ValidationError[] => issues.filter((e) => e.severity === 'warning')

export interface ValidateOptions {
  /** layout.target として有効な、組み込み以外のレイアウト id（他のレイアウトなど）。 */
  knownLayoutIds?: readonly string[]
}

type Obj = Record<string, unknown>

const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v)
const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const len = (s: string) => [...s].length

class Collector {
  errors: ValidationError[] = []
  add(path: string, message: string) {
    this.errors.push({ path, message, severity: 'error' })
  }
  /** 見た目の値の問題。iOS ではそのプロパティだけ無視される（書き出しはブロックしない）。 */
  warn(path: string, message: string) {
    this.errors.push({ path, message, severity: 'warning' })
  }
  /** 許可されていないフィールドを報告する。 */
  extra(o: Obj, allowed: readonly string[], path: string) {
    for (const k of Object.keys(o)) {
      if (!allowed.includes(k)) this.add(join(path, k), `未知のフィールド「${k}」は指定できません`)
    }
  }
}

const join = (path: string, key: string) => (path ? `${path}.${key}` : key)

function str(c: Collector, v: unknown, path: string, min: number, max: number, what: string): v is string {
  if (typeof v !== 'string') {
    c.add(path, `${what}は文字列で指定してください`)
    return false
  }
  const n = len(v)
  if (n < min) c.add(path, min === 1 ? `${what}は空にできません` : `${what}は ${min} 文字以上にしてください`)
  else if (n > max) c.add(path, `${what}は ${max} 文字以内にしてください（現在 ${n} 文字）`)
  else return true
  return false
}

/** icon の書式。`iconify:` / `sf:` で始まるものだけ検査し、それ以外は従来どおり SF Symbols 名として通す。 */
function validateIcon(c: Collector, v: unknown, path: string) {
  if (!str(c, v, path, 0, LIMITS.iconMax, 'icon')) return
  const reason = iconFormatError(v)
  if (reason) c.add(path, reason)
}

function validateAction(c: Collector, v: unknown, path: string) {
  if (typeof v !== 'string' || !ACTION_PATTERN.test(v)) {
    c.add(path, 'action は英字で始まる英数字（32 文字以内）で指定してください（例: space, backspace）')
  }
}

function validateSteps(c: Collector, v: unknown, path: string) {
  if (!Array.isArray(v)) return c.add(path, 'steps は配列で指定してください')
  if (v.length < 1) c.add(path, 'steps は 1 つ以上必要です')
  if (v.length > LIMITS.stepsMax) c.add(path, `steps は ${LIMITS.stepsMax} 個以内にしてください`)
  v.forEach((s, i) => {
    const p = `${path}[${i}]`
    if (!isObj(s)) return c.add(p, '手順はオブジェクトで指定してください')
    const hasA = 'action' in s
    const hasT = 'text' in s
    if (hasA === hasT) return c.add(p, '手順は action か text のどちらか一方だけを指定してください')
    c.extra(s, [hasA ? 'action' : 'text'], p)
    if (hasA) validateAction(c, s.action, join(p, 'action'))
    else str(c, s.text, join(p, 'text'), 1, LIMITS.shortcutTextMax, 'text')
  })
}

function validateCommon(c: Collector, k: Obj, path: string, isPopup: boolean) {
  if (isPopup) {
    if (k.id !== undefined && !(typeof k.id === 'string' && KEY_ID_PATTERN.test(k.id)))
      c.add(join(path, 'id'), 'id は英数字・_ . - で 32 文字以内にしてください')
    str(c, k.label, join(path, 'label'), 1, LIMITS.labelMax, 'label')
    return
  }
  if (typeof k.id !== 'string' || !KEY_ID_PATTERN.test(k.id))
    c.add(join(path, 'id'), 'id は英数字・_ . - で 32 文字以内にしてください')
  if (k.width !== undefined && !(isNum(k.width) && k.width > 0 && k.width <= LIMITS.widthMax))
    c.add(join(path, 'width'), `width は 0 より大きく ${LIMITS.widthMax} 以下の数にしてください`)
  if (k.label !== undefined) str(c, k.label, join(path, 'label'), 0, LIMITS.labelMax, 'label')
  if (k.icon !== undefined) validateIcon(c, k.icon, join(path, 'icon'))
  if (k.role !== undefined && !(KEY_ROLES as readonly unknown[]).includes(k.role))
    c.add(join(path, 'role'), `role は ${KEY_ROLES.join(' / ')} のいずれかにしてください`)
  if (k.visible !== undefined) {
    const p = join(path, 'visible')
    const v = k.visible
    if (!isObj(v)) c.add(p, 'visible はオブジェクトで指定してください')
    else {
      c.extra(v, ['field', 'in'], p)
      str(c, v.field, join(p, 'field'), 1, 32, 'field')
      if (!Array.isArray(v.in) || v.in.length < 1 || v.in.length > 16 || v.in.some((x) => typeof x !== 'string' || len(x) > 32))
        c.add(join(p, 'in'), 'in は 1〜16 個の文字列（各 32 文字以内）の配列にしてください')
    }
  }
  if (k.longPress !== undefined) {
    const p = join(path, 'longPress')
    const lp = k.longPress
    if (!Array.isArray(lp)) c.add(p, 'longPress は配列で指定してください')
    else {
      if (lp.length < 1 || lp.length > LIMITS.longPressMax)
        c.add(p, `longPress は 1〜${LIMITS.longPressMax} 個にしてください（不要なら項目ごと削除）`)
      lp.forEach((pk, i) => validatePopup(c, pk, `${p}[${i}]`))
    }
  }
}

const COMMON_FIELDS = ['id', 'kind', 'width', 'label', 'icon', 'role', 'longPress', 'visible', 'look', 'states']

function validatePopup(c: Collector, pk: unknown, path: string) {
  if (!isObj(pk)) return c.add(path, '長押しの項目はオブジェクトで指定してください')
  const kind = pk.kind
  if (!(POPUP_KINDS as readonly unknown[]).includes(kind))
    return c.add(join(path, 'kind'), `長押しの kind は ${POPUP_KINDS.join(' / ')} のいずれかにしてください`)
  validateCommon(c, pk, path, true)
  if (kind === 'text') {
    c.extra(pk, ['id', 'label', 'kind', 'text'], path)
    str(c, pk.text, join(path, 'text'), 1, LIMITS.textMax, 'text')
  } else if (kind === 'action') {
    c.extra(pk, ['id', 'label', 'kind', 'action'], path)
    validateAction(c, pk.action, join(path, 'action'))
  } else {
    c.extra(pk, ['id', 'label', 'kind', 'steps'], path)
    validateSteps(c, pk.steps, join(path, 'steps'))
  }
}

function validateKey(c: Collector, k: unknown, path: string, targets: readonly string[]) {
  if (!isObj(k)) return c.add(path, 'キーはオブジェクトで指定してください')
  const kind = k.kind
  if (!(KEY_KINDS as readonly unknown[]).includes(kind))
    return c.add(join(path, 'kind'), `kind は ${KEY_KINDS.join(' / ')} のいずれかにしてください`)
  validateCommon(c, k, path, false)
  validateKeyLooks(c, k, path)
  switch (kind) {
    case 'text':
      c.extra(k, [...COMMON_FIELDS, 'text', 'shift'], path)
      str(c, k.text, join(path, 'text'), 1, LIMITS.textMax, 'text')
      if (k.shift !== undefined) str(c, k.shift, join(path, 'shift'), 1, LIMITS.textMax, 'shift')
      break
    case 'flick': {
      c.extra(k, [...COMMON_FIELDS, 'center', ...FLICK_DIRECTIONS, 'kana'], path)
      for (const d of ['center', ...FLICK_DIRECTIONS] as const) {
        const cell = k[d]
        const p = join(path, d)
        if (cell === undefined) {
          if (d === 'center' && k.kana === undefined) c.add(p, 'フリックには center（中央の文字）か kana が必要です')
          continue
        }
        if (!isObj(cell)) c.add(p, `${d} は { "text": "..." } で指定してください`)
        else {
          c.extra(cell, ['text'], p)
          str(c, cell.text, join(p, 'text'), 1, LIMITS.textMax, 'text')
        }
      }
      if (k.kana !== undefined) str(c, k.kana, join(path, 'kana'), 1, LIMITS.kanaMax, 'kana')
      break
    }
    case 'toggle': {
      c.extra(k, [...COMMON_FIELDS, 'cycle'], path)
      const p = join(path, 'cycle')
      if (!Array.isArray(k.cycle)) c.add(p, 'cycle は文字列の配列で指定してください')
      else {
        if (k.cycle.length < 1 || k.cycle.length > LIMITS.cycleMax)
          c.add(p, `cycle は 1〜${LIMITS.cycleMax} 要素にしてください`)
        k.cycle.forEach((s, i) => str(c, s, `${p}[${i}]`, 1, LIMITS.textMax, 'cycle の要素'))
      }
      break
    }
    case 'action':
      c.extra(k, [...COMMON_FIELDS, 'action'], path)
      validateAction(c, k.action, join(path, 'action'))
      break
    case 'shortcut':
      c.extra(k, [...COMMON_FIELDS, 'steps'], path)
      validateSteps(c, k.steps, join(path, 'steps'))
      break
    case 'layout':
      c.extra(k, [...COMMON_FIELDS, 'target'], path)
      if (typeof k.target !== 'string' || !LAYOUT_ID_PATTERN.test(k.target))
        c.add(join(path, 'target'), 'target は切り替え先のレイアウト id（英小文字・数字・- _）で指定してください')
      else if (!targets.includes(k.target))
        c.add(join(path, 'target'), `切り替え先「${k.target}」は存在するレイアウトでも組み込み名でもありません`)
      break
    case 'spacer':
      c.extra(k, COMMON_FIELDS, path)
      break
  }
}

// ---- 見た目（look）の検証。docs/keyboard-style-spec.md §5.5 ----------------------------------------------------------
// 見た目の値の問題は warning（iOS はそのプロパティだけ無視する）。資源の上限（assets・rules の個数）は error。

const LAYOUT_LOOK_FIELDS = ['look', 'roles', 'rules', 'metrics', 'background', 'assets'] as const
const hasOwn = (o: object, k: string) => Object.prototype.hasOwnProperty.call(o, k)

/** look の中身。allowed を渡すと、その名前のプロパティだけ受け付ける（background 用）。 */
function validateLookObject(c: Collector, v: unknown, path: string, allowsContent: boolean, allowed?: readonly string[]) {
  if (!isObj(v)) return c.warn(path, '見た目の指定はオブジェクトで指定してください（この項目は無視されます）')
  for (const [name, value] of Object.entries(v)) {
    const p = join(path, name)
    if (!hasOwn(LOOK_PROPS, name)) {
      c.warn(p, `未知のプロパティ「${name}」は無視されます`)
      continue
    }
    if (allowed && !allowed.includes(name)) {
      c.warn(p, `「${name}」は背景には使えません（無視されます）`)
      continue
    }
    if (!allowsContent && (LOOK_CONTENT_PROPS as readonly string[]).includes(name)) {
      c.warn(p, `「${name}」は states / rules の中だけで有効です（ここでは無視されます）`)
      continue
    }
    const r = checkLookValue(name, value)
    if (!r.ok) c.warn(p, `「${name}」は無視されます: ${r.reason}`)
  }
}

/** states 内の見た目（look ＋ holdMs）。 */
function validateStateLook(c: Collector, v: unknown, path: string) {
  if (!isObj(v)) return c.warn(path, '状態の見た目はオブジェクトで指定してください（この項目は無視されます）')
  const { holdMs, ...look } = v
  if (holdMs !== undefined) validateHoldMs(c, holdMs, join(path, 'holdMs'))
  validateLookObject(c, look, path, true)
}

function validateHoldMs(c: Collector, v: unknown, path: string) {
  if (!(typeof v === 'number' && Number.isFinite(v) && v >= LOOK_LIMITS.holdMsMin && v <= LOOK_LIMITS.holdMsMax))
    c.warn(path, `holdMs は ${LOOK_LIMITS.holdMsMin}〜${LOOK_LIMITS.holdMsMax} の数で指定してください（無視されます）`)
}

/** キーの look と states。 */
function validateKeyLooks(c: Collector, k: Obj, path: string) {
  if (k.look !== undefined) validateLookObject(c, k.look, join(path, 'look'), false)
  if (k.states === undefined) return
  const p = join(path, 'states')
  if (!isObj(k.states)) return c.warn(p, 'states はオブジェクトで指定してください（この項目は無視されます）')
  for (const [state, look] of Object.entries(k.states)) {
    if (!(LOOK_STATES as readonly string[]).includes(state)) c.warn(join(p, state), `未知の状態「${state}」は無視されます`)
    else validateStateLook(c, look, join(p, state))
  }
}

function validateRules(c: Collector, v: unknown) {
  if (!Array.isArray(v)) return c.warn('rules', 'rules は配列で指定してください（この項目は無視されます）')
  if (v.length > LOOK_LIMITS.rulesMax) c.add('rules', `rules は ${LOOK_LIMITS.rulesMax} 個以内にしてください（現在 ${v.length}）`)
  v.forEach((rule, i) => {
    const p = `rules[${i}]`
    if (!isObj(rule)) return c.warn(p, 'rule はオブジェクトで指定してください（無視されます）')
    for (const k of Object.keys(rule)) {
      if (!['when', 'target', 'holdMs', 'look'].includes(k)) c.warn(join(p, k), `未知のフィールド「${k}」は無視されます`)
    }
    if (!(LOOK_STATES as readonly unknown[]).includes(rule.when)) {
      c.warn(join(p, 'when'), `when は ${LOOK_STATES.join(' / ')} のいずれかにしてください（この rule は無視されます）`)
    }
    if (rule.target !== undefined || hasOwn(rule, 'target')) {
      if (!parseTarget(rule)) {
        c.warn(join(p, 'target'), 'target は "all" / "others" / { keys } / { roles } / { actions } のいずれか 1 つにしてください（この rule は無視されます）')
      } else if (isObj(rule.target) && Array.isArray(rule.target.keys) && rule.target.keys.length > LOOK_LIMITS.targetKeysMax) {
        c.add(join(p, 'target.keys'), `target.keys は ${LOOK_LIMITS.targetKeysMax} 個以内にしてください（現在 ${rule.target.keys.length}）`)
      } else if (isObj(rule.target) && Array.isArray(rule.target.roles)) {
        for (const r of rule.target.roles) {
          if (!(KEY_ROLES as readonly unknown[]).includes(r)) c.warn(join(p, 'target.roles'), `未知の役割「${String(r)}」は無視されます`)
        }
      }
    }
    if (rule.holdMs !== undefined) validateHoldMs(c, rule.holdMs, join(p, 'holdMs'))
    if (rule.look === undefined) c.warn(join(p, 'look'), 'look がありません（この rule は何も変えません）')
    else validateLookObject(c, rule.look, join(p, 'look'), true)
  })
}

function validateMetrics(c: Collector, v: unknown) {
  if (!isObj(v)) return c.warn('metrics', 'metrics はオブジェクトで指定してください（この項目は無視されます）')
  for (const [name, value] of Object.entries(v)) {
    const p = join('metrics', name)
    if (!hasOwn(METRICS_PROPS, name)) {
      c.warn(p, `未知のプロパティ「${name}」は無視されます`)
      continue
    }
    const range = METRICS_PROPS[name as keyof typeof METRICS_PROPS]
    if (!(typeof value === 'number' && Number.isFinite(value) && value >= range.min && value <= range.max))
      c.warn(p, `「${name}」は ${range.min}〜${range.max} の数で指定してください（無視されます）`)
  }
}

function validateAssets(c: Collector, v: unknown) {
  if (!isObj(v)) return c.add('assets', 'assets は { "画像 id": "data URI" } のオブジェクトで指定してください')
  const ids = Object.keys(v).sort()
  if (ids.length > ASSET_LIMITS.countMax) c.add('assets', `画像は ${ASSET_LIMITS.countMax} 枚以内にしてください（現在 ${ids.length}）`)
  let total = 0
  for (const id of ids) {
    const p = join('assets', id)
    if (!ASSET_ID_PATTERN.test(id)) {
      c.add(p, '画像 id は英小文字・数字・- _ で始まりが英小文字か数字、32 文字以内にしてください')
      continue
    }
    const bytes = dataUriBytes(v[id])
    if (bytes === undefined) c.add(p, `画像は ${ASSET_LIMITS.mimeTypes.join(' か ')} の base64 data URI で指定してください`)
    else if (bytes > ASSET_LIMITS.bytesMax)
      c.add(p, `画像は 1 枚 ${ASSET_LIMITS.bytesMax / 1024} KB 以内にしてください（現在 ${Math.ceil(bytes / 1024)} KB）`)
    else {
      total += bytes
      const d = imageDimensions(v[id])
      if (d && Math.max(d.width, d.height) > ASSET_LIMITS.longSideMax)
        c.warn(p, `画像の長辺は ${ASSET_LIMITS.longSideMax}px 以内を推奨します（現在 ${Math.max(d.width, d.height)}px。アプリは警告を出しますが取り込めます）`)
    }
  }
  if (total > ASSET_LIMITS.totalMax)
    c.add('assets', `画像の合計は ${ASSET_LIMITS.totalMax / 1024} KB 以内にしてください（現在 ${Math.ceil(total / 1024)} KB）`)
}

/** レイアウト直下の look 系（look / roles / rules / metrics / background / assets）。 */
function validateLayoutLooks(c: Collector, layout: Obj) {
  if (layout.look !== undefined) validateLookObject(c, layout.look, 'look', false)
  if (layout.roles !== undefined) {
    if (!isObj(layout.roles)) c.warn('roles', 'roles はオブジェクトで指定してください（この項目は無視されます）')
    else
      for (const [role, look] of Object.entries(layout.roles)) {
        if (!(KEY_ROLES as readonly string[]).includes(role)) c.warn(join('roles', role), `未知の役割「${role}」は無視されます`)
        else validateLookObject(c, look, join('roles', role), false)
      }
  }
  if (layout.rules !== undefined) validateRules(c, layout.rules)
  if (layout.metrics !== undefined) validateMetrics(c, layout.metrics)
  if (layout.background !== undefined) validateLookObject(c, layout.background, 'background', false, BACKGROUND_PROPS)
  if (layout.assets !== undefined) validateAssets(c, layout.assets)
}

/** レイアウトを検証して問題の一覧（error と warning）を返す。error が無ければ有効（warning は書き出しをブロックしない）。 */
export function validateLayout(value: unknown, options: ValidateOptions = {}): ValidationError[] {
  const c = new Collector()
  if (!isObj(value)) {
    c.add('', 'レイアウトは JSON オブジェクトである必要があります')
    return c.errors
  }
  c.extra(value, ['$schema', 'schema', 'id', 'name', 'mode', 'rows', 'toolbar', ...LAYOUT_LOOK_FIELDS], '')
  if (value.schema !== SCHEMA_VERSION)
    c.add('schema', `schema は ${SCHEMA_VERSION} にしてください${value.schema === 1 ? '（schema 1 は未対応です。web/scripts/migrate-layout-v2.mjs で移行してください）' : ''}`)
  if (typeof value.id !== 'string' || !LAYOUT_ID_PATTERN.test(value.id))
    c.add('id', 'id は英小文字・数字・- _ で始まりが英小文字か数字、64 文字以内にしてください')
  str(c, value.name, 'name', 1, LIMITS.nameMax, 'name')
  if (!(LAYOUT_MODES as readonly unknown[]).includes(value.mode))
    c.add('mode', `mode は ${LAYOUT_MODES.join(' / ')} のいずれかにしてください`)

  const targets = [...BUILTIN_LAYOUT_IDS, ...(options.knownLayoutIds ?? []), ...(typeof value.id === 'string' ? [value.id] : [])]
  const rows = value.rows
  if (!Array.isArray(rows)) {
    c.add('rows', 'rows は配列で指定してください')
    return c.errors
  }
  validateLayoutLooks(c, value)
  if (rows.length < 1 || rows.length > LIMITS.rowsMax) c.add('rows', `行数は 1〜${LIMITS.rowsMax} にしてください（現在 ${rows.length}）`)

  const heightSum = rows.reduce((a: number, r) => a + (isObj(r) && isNum(r.height) ? r.height : 1), 0)
  if (heightSum > LIMITS.rowHeightSumMax)
    c.add('rows', `行の高さ比の合計は ${LIMITS.rowHeightSumMax} 以下にしてください（現在 ${heightSum}）`)

  const seen = new Map<string, string>()
  rows.forEach((row, ri) => {
    const rp = `rows[${ri}]`
    if (!isObj(row)) return c.add(rp, '行はオブジェクトで指定してください')
    c.extra(row, ['height', 'keys', 'look'], rp)
    if (row.look !== undefined) validateLookObject(c, row.look, join(rp, 'look'), false)
    if (row.height !== undefined && !(isNum(row.height) && row.height > 0 && row.height <= LIMITS.rowHeightMax))
      c.add(join(rp, 'height'), `height は 0 より大きく ${LIMITS.rowHeightMax} 以下の数にしてください`)
    if (!Array.isArray(row.keys)) return c.add(join(rp, 'keys'), 'keys は配列で指定してください')
    if (row.keys.length < 1) c.add(join(rp, 'keys'), '行には 1 つ以上のキーが必要です')
    if (row.keys.length > LIMITS.keysPerRowMax) c.add(join(rp, 'keys'), `1 行のキーは ${LIMITS.keysPerRowMax} 個以内にしてください`)
    validateKeyList(c, row.keys, rp, targets, seen)
  })
  if (value.toolbar !== undefined) validateToolbar(c, value.toolbar, targets, seen)
  return c.errors
}

/** キー列の各キーの検証と、id の一意性・行内の幅の合計。 */
function validateKeyList(c: Collector, keys: unknown[], rp: string, targets: readonly string[], seen: Map<string, string>) {
  let sum = 0
  keys.forEach((k, ki) => {
    const kp = `${rp}.keys[${ki}]`
    validateKey(c, k, kp, targets)
    if (isObj(k)) {
      sum += isNum(k.width) ? k.width : 1
      if (typeof k.id === 'string') {
        const prev = seen.get(k.id)
        if (prev) c.add(join(kp, 'id'), `キー id の重複: 「${k.id}」が ${prev} と重複しています`)
        else seen.set(k.id, kp)
      }
    }
  })
  if (keys.length > 0 && (sum < LIMITS.rowWidthMin || sum > LIMITS.rowWidthMax))
    c.add(join(rp, 'keys'), rp === 'toolbar' ? `toolbar の幅の合計が範囲外です（${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください。現在 ${round(sum)}）` : `行内の幅の合計は ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください（現在 ${round(sum)}）`)
}

/** toolbar（候補バーが空のときに出る 1 行）。キーは rows と同じ型で、行の height は持たない。 */
function validateToolbar(c: Collector, tb: unknown, targets: readonly string[], seen: Map<string, string>) {
  if (!isObj(tb)) return c.add('toolbar', 'toolbar はオブジェクトで指定してください')
  c.extra(tb, ['keys', 'look'], 'toolbar')
  if (tb.look !== undefined) validateLookObject(c, tb.look, 'toolbar.look', false)
  if (!Array.isArray(tb.keys)) return c.add('toolbar.keys', 'keys は配列で指定してください')
  if (tb.keys.length < LIMITS.toolbarKeysMin || tb.keys.length > LIMITS.toolbarKeysMax)
    c.add('toolbar.keys', `toolbar のキーは ${LIMITS.toolbarKeysMin}〜${LIMITS.toolbarKeysMax} 個にしてください（現在 ${tb.keys.length}）。不要なら toolbar ごと削除`)
  validateKeyList(c, tb.keys, 'toolbar', targets, seen)
}

const round = (n: number) => Math.round(n * 1000) / 1000

/** 型ガードとして使う。error が無ければ有効（warning は問わない）。 */
export function isValidLayout(value: unknown, options?: ValidateOptions): value is KeyboardLayout {
  return errorsOf(validateLayout(value, options)).length === 0
}

export type ParseResult =
  | { ok: true; layout: KeyboardLayout; warnings: ValidationError[] }
  | { ok: false; errors: ValidationError[] }

/** JSON 文字列を読み込んで検証する。構文エラーも ValidationError で返す。 */
export function parseLayoutJSON(text: string, options?: ValidateOptions): ParseResult {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch (e) {
    return { ok: false, errors: [{ path: '', message: `JSON として読み込めません: ${(e as Error).message}` }] }
  }
  const issues = validateLayout(data, options)
  const errors = errorsOf(issues)
  return errors.length ? { ok: false, errors } : { ok: true, layout: data as KeyboardLayout, warnings: warningsOf(issues) }
}

/** `rows[0].keys[2].text` のようなパスを「1 行目の 3 番目のキー」に読み替える。 */
export function describePath(path: string): string {
  if (!path) return 'レイアウト全体'
  return path
    .replace(/^toolbar(?=\.|$)/, 'ツールバー')
    .replace(/rows\[(\d+)\]/g, (_, n) => `${Number(n) + 1} 行目`)
    .replace(/\.keys\[(\d+)\]/g, (_, n) => ` の ${Number(n) + 1} 番目のキー`)
    .replace(/\.longPress\[(\d+)\]/g, (_, n) => ` の長押し ${Number(n) + 1} 番目`)
    .replace(/\.steps\[(\d+)\]/g, (_, n) => ` の手順 ${Number(n) + 1}`)
    .replace(/\.cycle\[(\d+)\]/g, (_, n) => ` の cycle ${Number(n) + 1}`)
    .replace(/rules\[(\d+)\]/g, (_, n) => `rules ${Number(n) + 1} 番目`)
}
