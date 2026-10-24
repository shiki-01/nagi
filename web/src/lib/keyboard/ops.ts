// レイアウトの編集操作。すべて純粋関数（入力を変更せず新しい値を返す）。

import { LIMITS } from './types'
import type { Key, KeyKind, KeyboardLayout, LayoutMode, Row } from './types'

export function sampleLayout(): KeyboardLayout {
  return {
    schema: 2,
    id: 'sample',
    name: 'サンプル（QWERTY 1 行）',
    mode: 'alpha',
    rows: [
      {
        keys: ['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'].map(
          (c): Key => ({ id: c, kind: 'text', label: c, text: c, shift: c.toUpperCase() })
        )
      },
      {
        keys: [
          { id: 'shift', kind: 'action', action: 'shift', icon: 'shift', label: '⇧', role: 'function', width: 1.5 },
          { id: 'space', kind: 'action', action: 'space', label: 'space', width: 5 },
          { id: 'enter', kind: 'action', action: 'enter', label: '改行', role: 'accent', width: 2 }
        ]
      }
    ]
  }
}

export function clone<T>(v: T): T {
  return structuredClone(v)
}

/** `base`（無ければ `base-2`, `base-3` …）で `used` に無い id を作る。 */
export function uniqueId(base: string, used: Iterable<string>): string {
  const set = new Set(used)
  if (!set.has(base)) return base
  for (let n = 2; ; n++) {
    const cand = `${base}-${n}`
    if (!set.has(cand)) return cand
  }
}

export function allKeyIds(layout: KeyboardLayout): string[] {
  return layout.rows.flatMap((r) => r.keys.map((k) => k.id))
}

export function newLayout(existingIds: readonly string[], mode: LayoutMode = 'custom'): KeyboardLayout {
  return {
    schema: 2,
    id: uniqueId('my-layout', existingIds),
    name: '新しいレイアウト',
    mode,
    rows: [{ keys: [{ id: 'key-1', kind: 'text', label: 'a', text: 'a', shift: 'A' }] }]
  }
}

export function duplicateLayout(layout: KeyboardLayout, existingIds: readonly string[]): KeyboardLayout {
  const copy = clone(layout)
  copy.id = uniqueId(`${layout.id}-copy`.slice(0, 64), existingIds)
  copy.name = `${layout.name} のコピー`.slice(0, 64)
  return copy
}

/** 他のレイアウトと id が衝突しない形で取り込む。 */
export function withUniqueLayoutId(layout: KeyboardLayout, existingIds: readonly string[]): KeyboardLayout {
  const copy = clone(layout)
  copy.id = uniqueId(layout.id, existingIds)
  return copy
}

export function rowWidth(row: Row): number {
  return row.keys.reduce((s, k) => s + (k.width ?? 1), 0)
}

export function findKey(layout: KeyboardLayout, keyId: string): { row: number; index: number; key: Key } | undefined {
  for (let row = 0; row < layout.rows.length; row++) {
    const index = layout.rows[row].keys.findIndex((k) => k.id === keyId)
    if (index >= 0) return { row, index, key: layout.rows[row].keys[index] }
  }
  return undefined
}

function clamp(n: number, lo: number, hi: number) {
  return Math.max(lo, Math.min(hi, n))
}

export function defaultKey(kind: KeyKind, id: string): Key {
  switch (kind) {
    case 'text':
      return { id, kind, label: 'a', text: 'a' }
    case 'flick':
      return { id, kind, label: 'あ', center: { text: 'あ' }, up: { text: 'う' }, down: { text: 'お' }, left: { text: 'い' }, right: { text: 'え' } }
    case 'toggle':
      return { id, kind, label: 'ab', cycle: ['a', 'b'] }
    case 'action':
      return { id, kind, label: '空白', action: 'space', role: 'function' }
    case 'shortcut':
      return { id, kind, label: '定型', steps: [{ text: 'ありがとうございます' }] }
    case 'layout':
      return { id, kind, label: '123', target: 'numbers', role: 'function' }
    case 'spacer':
      return { id, kind }
  }
}

/** kind を切り替える。共通フィールド（id・幅・見た目・長押し）は残し、kind 固有のフィールドは既定値にする。 */
export function changeKind(key: Key, kind: KeyKind): Key {
  if (key.kind === kind) return key
  const fresh = defaultKey(kind, key.id)
  const out: Record<string, unknown> = { ...fresh }
  if (key.width !== undefined) out.width = key.width
  if (key.role !== undefined) out.role = key.role
  if (key.icon !== undefined) out.icon = key.icon
  if (key.longPress !== undefined && kind !== 'spacer') out.longPress = key.longPress
  if (key.visible !== undefined) out.visible = key.visible
  // 見た目の指定（look / states）は kind に依らないので残す
  if (key.look !== undefined) out.look = key.look
  if (key.states !== undefined) out.states = key.states
  if (key.label !== undefined && kind !== 'spacer') out.label = key.label
  return out as unknown as Key
}

export function addRow(layout: KeyboardLayout, index = layout.rows.length): KeyboardLayout {
  const next = clone(layout)
  const id = uniqueId('key', allKeyIds(next))
  next.rows.splice(clamp(index, 0, next.rows.length), 0, { keys: [{ id, kind: 'text', label: 'a', text: 'a' }] })
  return next
}

export function removeRow(layout: KeyboardLayout, index: number): KeyboardLayout {
  if (index < 0 || index >= layout.rows.length) return layout
  const next = clone(layout)
  next.rows.splice(index, 1)
  return next
}

export function moveRow(layout: KeyboardLayout, from: number, to: number): KeyboardLayout {
  if (from === to || from < 0 || from >= layout.rows.length) return layout
  const next = clone(layout)
  const [row] = next.rows.splice(from, 1)
  next.rows.splice(clamp(to, 0, next.rows.length), 0, row)
  return next
}

export function updateRow(layout: KeyboardLayout, index: number, patch: Partial<Pick<Row, 'height'>>): KeyboardLayout {
  const next = clone(layout)
  const row = next.rows[index]
  if (!row) return layout
  if ('height' in patch) {
    if (patch.height === undefined) delete row.height
    else row.height = patch.height
  }
  return next
}

export function addKey(layout: KeyboardLayout, rowIndex: number, kind: KeyKind = 'text', index?: number): { layout: KeyboardLayout; keyId: string } {
  const next = clone(layout)
  const row = next.rows[rowIndex]
  if (!row) return { layout, keyId: '' }
  const id = uniqueId('key', allKeyIds(next))
  row.keys.splice(clamp(index ?? row.keys.length, 0, row.keys.length), 0, defaultKey(kind, id))
  return { layout: next, keyId: id }
}

export function duplicateKey(layout: KeyboardLayout, keyId: string): { layout: KeyboardLayout; keyId: string } {
  const found = findKey(layout, keyId)
  if (!found) return { layout, keyId: '' }
  const next = clone(layout)
  const copy = clone(found.key)
  copy.id = uniqueId(`${keyId}-copy`.slice(0, 32), allKeyIds(next))
  next.rows[found.row].keys.splice(found.index + 1, 0, copy)
  return { layout: next, keyId: copy.id }
}

export function removeKey(layout: KeyboardLayout, keyId: string): KeyboardLayout {
  const found = findKey(layout, keyId)
  if (!found) return layout
  const next = clone(layout)
  next.rows[found.row].keys.splice(found.index, 1)
  return next
}

/** キーを移動する。`toIndex` は取り除いた後の行での挿入位置。行をまたいでもよい。 */
export function moveKey(layout: KeyboardLayout, keyId: string, toRow: number, toIndex: number): KeyboardLayout {
  const found = findKey(layout, keyId)
  if (!found || toRow < 0 || toRow >= layout.rows.length) return layout
  const next = clone(layout)
  const [key] = next.rows[found.row].keys.splice(found.index, 1)
  const target = next.rows[toRow].keys
  const at = clamp(toIndex, 0, target.length)
  if (toRow === found.row && at === found.index) return layout
  target.splice(at, 0, key)
  return next
}

/** 同じ行の中で左右に 1 つ動かす。 */
export function shiftKey(layout: KeyboardLayout, keyId: string, delta: -1 | 1): KeyboardLayout {
  const found = findKey(layout, keyId)
  if (!found) return layout
  const to = found.index + delta
  if (to < 0 || to >= layout.rows[found.row].keys.length) return layout
  return moveKey(layout, keyId, found.row, to)
}

export function replaceKey(layout: KeyboardLayout, keyId: string, key: Key): KeyboardLayout {
  const found = findKey(layout, keyId)
  if (!found) return layout
  const next = clone(layout)
  next.rows[found.row].keys[found.index] = key
  return next
}

// --- キャンバス編集用の操作（複数キーの移動・行間への挿入・矢印キーによる移動など） ---

/** 移動先。`row`/`index` は既存の行への挿入（index は移動するキーを除いた行での位置）、`newRow` は行の挿入位置（0〜行数）。 */
export type MoveDest = { row: number; index: number } | { newRow: number }

export interface MoveResult {
  layout: KeyboardLayout
  /** 上限などで実行できなかった理由。 */
  error?: string
}

/** 指定した id のキーを、レイアウト上の並び順（行→列）で返す。存在しない id は無視。 */
function inDocOrder(layout: KeyboardLayout, keyIds: readonly string[]): string[] {
  const want = new Set(keyIds)
  return layout.rows.flatMap((r) => r.keys.filter((k) => want.has(k.id)).map((k) => k.id))
}

/**
 * 複数のキーをまとめて移動する（並び順は保つ）。行をまたいでよく、元の行が空になったらその行は削除する。
 * 行数・1 行のキー数の上限を超える場合は `error` を付けて入力をそのまま返す。
 */
export function moveKeysChecked(layout: KeyboardLayout, keyIds: readonly string[], dest: MoveDest): MoveResult {
  const order = inDocOrder(layout, keyIds)
  if (order.length === 0) return { layout }
  if ('row' in dest && (dest.row < 0 || dest.row >= layout.rows.length)) return { layout }
  const next = clone(layout)
  const want = new Set(order)
  const moved: Key[] = []
  for (const row of next.rows) {
    const keep: Key[] = []
    for (const k of row.keys) (want.has(k.id) ? moved : keep).push(k)
    row.keys = keep
  }
  // moved は行→列の順に集めているので、元の並び順になっている。
  if ('row' in dest) {
    const target = next.rows[dest.row]
    target.keys.splice(clamp(dest.index, 0, target.keys.length), 0, ...moved)
  } else {
    next.rows.splice(clamp(dest.newRow, 0, next.rows.length), 0, { keys: moved })
  }
  next.rows = next.rows.filter((r) => r.keys.length > 0)
  if (next.rows.length > LIMITS.rowsMax) return { layout, error: `行は最大 ${LIMITS.rowsMax} 行までです` }
  if (next.rows.some((r) => r.keys.length > LIMITS.keysPerRowMax)) return { layout, error: `1 行に置けるキーは最大 ${LIMITS.keysPerRowMax} 個までです` }
  if (JSON.stringify(next) === JSON.stringify(layout)) return { layout }
  return { layout: next }
}

export function moveKeys(layout: KeyboardLayout, keyIds: readonly string[], dest: MoveDest): KeyboardLayout {
  return moveKeysChecked(layout, keyIds, dest).layout
}

/** 複数のキーを削除する。空になった行は（最後の 1 行を除いて）削除する。 */
export function removeKeys(layout: KeyboardLayout, keyIds: readonly string[]): KeyboardLayout {
  const want = new Set(keyIds)
  if (!layout.rows.some((r) => r.keys.some((k) => want.has(k.id)))) return layout
  const next = clone(layout)
  for (const row of next.rows) row.keys = row.keys.filter((k) => !want.has(k.id))
  const kept = next.rows.filter((r) => r.keys.length > 0)
  next.rows = kept.length > 0 ? kept : [next.rows[0]]
  return next
}

/** 新しい行を `rowIndex` に作り、その 1 つ目のキーとして追加する。上限を超えるなら変更しない。 */
export function addKeyInNewRow(layout: KeyboardLayout, rowIndex: number, kind: KeyKind = 'text'): { layout: KeyboardLayout; keyId: string } {
  if (layout.rows.length >= LIMITS.rowsMax) return { layout, keyId: '' }
  const next = clone(layout)
  const id = uniqueId('key', allKeyIds(next))
  next.rows.splice(clamp(rowIndex, 0, next.rows.length), 0, { keys: [defaultKey(kind, id)] })
  return { layout: next, keyId: id }
}

/** 既存の行の `index` に挿入する。1 行のキー数の上限を超えるなら変更しない。 */
export function insertKey(layout: KeyboardLayout, rowIndex: number, index: number, kind: KeyKind): { layout: KeyboardLayout; keyId: string } {
  const row = layout.rows[rowIndex]
  if (!row || row.keys.length >= LIMITS.keysPerRowMax) return { layout, keyId: '' }
  return addKey(layout, rowIndex, kind, index)
}

export type Direction = 'left' | 'right' | 'up' | 'down'

/** 各キーの行内での [左端, 右端] を 0〜1 に正規化した値（行の幅の合計で割る）。 */
function spans(row: Row): { id: string; lo: number; hi: number }[] {
  const total = rowWidth(row) || 1
  let acc = 0
  return row.keys.map((k) => {
    const lo = acc / total
    acc += k.width ?? 1
    return { id: k.id, lo, hi: acc / total }
  })
}

/** 矢印キーで次に選ぶキー。左右は行をまたいで辿り、上下は隣の行で中心が最も近いキー。無ければ undefined。 */
export function navigateKey(layout: KeyboardLayout, fromId: string, dir: Direction): string | undefined {
  const found = findKey(layout, fromId)
  if (!found) return undefined
  const { row, index } = found
  if (dir === 'left' || dir === 'right') {
    const d = dir === 'left' ? -1 : 1
    const inRow = layout.rows[row].keys[index + d]
    if (inRow) return inRow.id
    const nr = layout.rows[row + d]
    if (!nr || nr.keys.length === 0) return undefined
    return (d < 0 ? nr.keys[nr.keys.length - 1] : nr.keys[0]).id
  }
  const target = layout.rows[row + (dir === 'up' ? -1 : 1)]
  if (!target || target.keys.length === 0) return undefined
  const me = spans(layout.rows[row])[index]
  const center = (me.lo + me.hi) / 2
  let best: { id: string; dist: number } | undefined
  for (const s of spans(target)) {
    const dist = center >= s.lo && center <= s.hi ? 0 : Math.min(Math.abs(center - s.lo), Math.abs(center - s.hi))
    if (!best || dist < best.dist) best = { id: s.id, dist }
  }
  return best?.id
}

/**
 * 選択中のキーを矢印キーで動かす。左右は同じ行の中で 1 つずつ、上下は隣の行へ（先頭のキーと同じ位置、または末尾へ）。
 * 端まで行っている場合や、上下で隣の行が無い場合は何もしない。
 */
export function nudgeKeys(layout: KeyboardLayout, keyIds: readonly string[], dir: Direction): KeyboardLayout {
  const order = inDocOrder(layout, keyIds)
  if (order.length === 0) return layout
  if (dir === 'up' || dir === 'down') {
    const first = findKey(layout, order[0])
    if (!first) return layout
    const toRow = first.row + (dir === 'up' ? -1 : 1)
    if (toRow < 0 || toRow >= layout.rows.length) return layout
    const want = new Set(order)
    const remaining = layout.rows[toRow].keys.filter((k) => !want.has(k.id)).length
    return moveKeys(layout, order, { row: toRow, index: Math.min(first.index, remaining) })
  }
  const want = new Set(order)
  const next = clone(layout)
  let changed = false
  for (const row of next.rows) {
    const ks = row.keys
    if (dir === 'left') {
      for (let i = 1; i < ks.length; i++)
        if (want.has(ks[i].id) && !want.has(ks[i - 1].id)) {
          ;[ks[i - 1], ks[i]] = [ks[i], ks[i - 1]]
          changed = true
        }
    } else {
      for (let i = ks.length - 2; i >= 0; i--)
        if (want.has(ks[i].id) && !want.has(ks[i + 1].id)) {
          ;[ks[i + 1], ks[i]] = [ks[i], ks[i + 1]]
          changed = true
        }
    }
  }
  return changed ? next : layout
}
