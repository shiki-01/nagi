// レイアウトの編集操作。すべて純粋関数（入力を変更せず新しい値を返す）。

import type { Key, KeyKind, KeyboardLayout, LayoutMode, Row } from './types'

export function sampleLayout(): KeyboardLayout {
  return {
    schema: 1,
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
          { id: 'shift', kind: 'action', action: 'shift', icon: 'shift', label: '⇧', style: 'function', width: 1.5 },
          { id: 'space', kind: 'action', action: 'space', label: 'space', width: 5 },
          { id: 'enter', kind: 'action', action: 'enter', label: '改行', style: 'accent', width: 2 }
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
    schema: 1,
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
      return { id, kind, label: '空白', action: 'space', style: 'function' }
    case 'shortcut':
      return { id, kind, label: '定型', steps: [{ text: 'ありがとうございます' }] }
    case 'layout':
      return { id, kind, label: '123', target: 'numbers', style: 'function' }
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
  if (key.style !== undefined) out.style = key.style
  if (key.icon !== undefined) out.icon = key.icon
  if (key.longPress !== undefined && kind !== 'spacer') out.longPress = key.longPress
  if (key.visible !== undefined) out.visible = key.visible
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
