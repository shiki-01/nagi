// config/keyboard/layout.schema.json を手書きで検証する（依存を増やさないため）。
// JSON Schema で表せない規則（行内の幅の合計、id の一意性、layout.target の存在）もここで見る。

import {
  ACTION_PATTERN,
  ICONIFY_ICON_PATTERN,
  SF_ICON_PATTERN,
  BUILTIN_LAYOUT_IDS,
  FLICK_DIRECTIONS,
  KEY_ID_PATTERN,
  KEY_KINDS,
  KEY_STYLES,
  LAYOUT_ID_PATTERN,
  LAYOUT_MODES,
  LIMITS,
  POPUP_KINDS,
  SCHEMA_VERSION
} from './types'
import type { KeyboardLayout } from './types'

export interface ValidationError {
  /** 例: `rows[0].keys[2].text`。レイアウト全体なら空文字。 */
  path: string
  message: string
}

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
    this.errors.push({ path, message })
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
  if (v.startsWith('iconify:') && !ICONIFY_ICON_PATTERN.test(v))
    c.add(path, 'iconify: の書式は iconify:<prefix>:<name>（小文字英数字とハイフン。例 iconify:mdi:home）にしてください')
  else if (v.startsWith('sf:') && !SF_ICON_PATTERN.test(v))
    c.add(path, 'sf: の後ろは SF Symbols 名（英数字と . _ -）にしてください')
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
  if (k.style !== undefined && !(KEY_STYLES as readonly unknown[]).includes(k.style))
    c.add(join(path, 'style'), `style は ${KEY_STYLES.join(' / ')} のいずれかにしてください`)
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

const COMMON_FIELDS = ['id', 'kind', 'width', 'label', 'icon', 'style', 'longPress', 'visible']

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

/** レイアウトを検証してエラー一覧を返す。空配列なら有効。 */
export function validateLayout(value: unknown, options: ValidateOptions = {}): ValidationError[] {
  const c = new Collector()
  if (!isObj(value)) {
    c.add('', 'レイアウトは JSON オブジェクトである必要があります')
    return c.errors
  }
  c.extra(value, ['$schema', 'schema', 'id', 'name', 'mode', 'rows'], '')
  if (value.schema !== SCHEMA_VERSION) c.add('schema', `schema は ${SCHEMA_VERSION} にしてください`)
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
  if (rows.length < 1 || rows.length > LIMITS.rowsMax) c.add('rows', `行数は 1〜${LIMITS.rowsMax} にしてください（現在 ${rows.length}）`)

  const heightSum = rows.reduce((a: number, r) => a + (isObj(r) && isNum(r.height) ? r.height : 1), 0)
  if (heightSum > LIMITS.rowHeightSumMax)
    c.add('rows', `行の高さ比の合計は ${LIMITS.rowHeightSumMax} 以下にしてください（現在 ${heightSum}）`)

  const seen = new Map<string, string>()
  rows.forEach((row, ri) => {
    const rp = `rows[${ri}]`
    if (!isObj(row)) return c.add(rp, '行はオブジェクトで指定してください')
    c.extra(row, ['height', 'keys'], rp)
    if (row.height !== undefined && !(isNum(row.height) && row.height > 0 && row.height <= LIMITS.rowHeightMax))
      c.add(join(rp, 'height'), `height は 0 より大きく ${LIMITS.rowHeightMax} 以下の数にしてください`)
    if (!Array.isArray(row.keys)) return c.add(join(rp, 'keys'), 'keys は配列で指定してください')
    if (row.keys.length < 1) c.add(join(rp, 'keys'), '行には 1 つ以上のキーが必要です')
    if (row.keys.length > LIMITS.keysPerRowMax) c.add(join(rp, 'keys'), `1 行のキーは ${LIMITS.keysPerRowMax} 個以内にしてください`)
    let sum = 0
    row.keys.forEach((k, ki) => {
      const kp = `${rp}.keys[${ki}]`
      validateKey(c, k, kp, targets)
      if (isObj(k)) {
        sum += isNum(k.width) ? k.width : 1
        if (typeof k.id === 'string') {
          const prev = seen.get(k.id)
          if (prev) c.add(join(kp, 'id'), `id「${k.id}」が ${prev} と重複しています`)
          else seen.set(k.id, kp)
        }
      }
    })
    if (row.keys.length > 0 && (sum < LIMITS.rowWidthMin || sum > LIMITS.rowWidthMax))
      c.add(join(rp, 'keys'), `行内の幅の合計は ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください（現在 ${round(sum)}）`)
  })
  return c.errors
}

const round = (n: number) => Math.round(n * 1000) / 1000

/** 型ガードとして使う。 */
export function isValidLayout(value: unknown, options?: ValidateOptions): value is KeyboardLayout {
  return validateLayout(value, options).length === 0
}

export type ParseResult = { ok: true; layout: KeyboardLayout } | { ok: false; errors: ValidationError[] }

/** JSON 文字列を読み込んで検証する。構文エラーも ValidationError で返す。 */
export function parseLayoutJSON(text: string, options?: ValidateOptions): ParseResult {
  let data: unknown
  try {
    data = JSON.parse(text)
  } catch (e) {
    return { ok: false, errors: [{ path: '', message: `JSON として読み込めません: ${(e as Error).message}` }] }
  }
  const errors = validateLayout(data, options)
  return errors.length ? { ok: false, errors } : { ok: true, layout: data as KeyboardLayout }
}

/** `rows[0].keys[2].text` のようなパスを「1 行目の 3 番目のキー」に読み替える。 */
export function describePath(path: string): string {
  if (!path) return 'レイアウト全体'
  return path
    .replace(/rows\[(\d+)\]/g, (_, n) => `${Number(n) + 1} 行目`)
    .replace(/\.keys\[(\d+)\]/g, (_, n) => ` の ${Number(n) + 1} 番目のキー`)
    .replace(/\.longPress\[(\d+)\]/g, (_, n) => ` の長押し ${Number(n) + 1} 番目`)
    .replace(/\.steps\[(\d+)\]/g, (_, n) => ` の手順 ${Number(n) + 1}`)
    .replace(/\.cycle\[(\d+)\]/g, (_, n) => ` の cycle ${Number(n) + 1}`)
}
