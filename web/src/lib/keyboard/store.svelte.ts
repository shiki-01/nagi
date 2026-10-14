// /custom-keyboard の編集状態。localStorage に自動保存し、undo/redo を持つ。

import { History } from './history'
import {
  addKey,
  addRow,
  changeKind,
  duplicateKey,
  duplicateLayout,
  findKey,
  moveKey,
  moveRow,
  newLayout,
  removeKey,
  removeRow,
  replaceKey,
  shiftKey,
  updateRow,
  withUniqueLayoutId,
  clone
} from './ops'
import { KEY_KINDS } from './types'
import type { Key, KeyKind, KeyboardLayout, Row } from './types'
import { validateLayout } from './validate'
import type { ValidationError } from './validate'

export const STORAGE_KEY = 'nagi.custom-keyboard.v1'

export interface EditorState {
  layouts: KeyboardLayout[]
  currentId: string
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

/** 編集途中の（スキーマ違反を含む）レイアウトも保存・復元できるよう、描画に必要な最低限の形だけ見る。 */
export function looksLikeLayout(v: unknown): v is KeyboardLayout {
  if (!isObj(v) || typeof v.id !== 'string' || typeof v.name !== 'string' || !Array.isArray(v.rows)) return false
  return v.rows.every(
    (r) =>
      isObj(r) &&
      Array.isArray(r.keys) &&
      r.keys.every((k) => isObj(k) && typeof k.id === 'string' && (KEY_KINDS as readonly unknown[]).includes(k.kind))
  )
}

/** 保存された JSON から状態を復元する。壊れていれば undefined。 */
export function restoreState(json: string | null): EditorState | undefined {
  if (!json) return undefined
  try {
    const data = JSON.parse(json)
    if (!isObj(data) || !Array.isArray(data.layouts)) return undefined
    const layouts = data.layouts.filter(looksLikeLayout)
    if (layouts.length === 0) return undefined
    const currentId = typeof data.currentId === 'string' && layouts.some((l) => l.id === data.currentId) ? data.currentId : layouts[0].id
    return { layouts, currentId }
  } catch {
    return undefined
  }
}

export class EditorStore {
  #history: History<EditorState>
  state = $state.raw<EditorState>({ layouts: [], currentId: '' })
  selectedKeyId = $state<string | null>(null)
  saveError = $state<string | null>(null)
  /** 履歴の変化をリアクティブにするためのカウンタ。 */
  #rev = $state(0)

  constructor(initial: EditorState) {
    this.#history = new History(initial)
    this.state = initial
  }

  get layout(): KeyboardLayout {
    return this.state.layouts.find((l) => l.id === this.state.currentId) ?? this.state.layouts[0]
  }
  get canUndo(): boolean {
    void this.#rev
    return this.#history.canUndo
  }
  get canRedo(): boolean {
    void this.#rev
    return this.#history.canRedo
  }
  get selectedKey(): Key | undefined {
    return this.selectedKeyId !== null ? findKey(this.layout, this.selectedKeyId)?.key : undefined
  }
  get errors(): ValidationError[] {
    const errors = validateLayout(this.layout, { knownLayoutIds: this.state.layouts.map((l) => l.id) })
    if (this.state.layouts.filter((l) => l.id === this.layout.id).length > 1)
      errors.push({ path: 'id', message: `レイアウト id「${this.layout.id}」が他のレイアウトと重複しています` })
    return errors
  }

  #apply(next: EditorState, coalesceKey?: string) {
    this.#history.push(next, coalesceKey)
    this.#sync()
  }
  #sync() {
    this.state = this.#history.value
    this.#rev++
    if (this.selectedKeyId !== null && !findKey(this.layout, this.selectedKeyId)) this.selectedKeyId = null
    this.#persist()
  }
  #persist() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.state))
      this.saveError = null
    } catch (e) {
      this.saveError = `ブラウザへの自動保存に失敗しました: ${(e as Error).message}`
    }
  }

  /** 現在のレイアウトを差し替える。 */
  edit(next: KeyboardLayout, coalesceKey?: string) {
    const layouts = this.state.layouts.map((l) => (l.id === this.state.currentId ? next : l))
    this.#apply({ layouts, currentId: next.id }, coalesceKey)
  }

  undo() {
    this.#history.undo()
    this.#sync()
  }
  redo() {
    this.#history.redo()
    this.#sync()
  }

  // --- レイアウト一覧 ---
  select(id: string) {
    if (!this.state.layouts.some((l) => l.id === id)) return
    this.selectedKeyId = null
    this.#apply({ ...this.state, currentId: id })
  }
  create() {
    const l = newLayout(this.state.layouts.map((x) => x.id))
    this.selectedKeyId = null
    this.#apply({ layouts: [...this.state.layouts, l], currentId: l.id })
  }
  duplicate() {
    const l = duplicateLayout(this.layout, this.state.layouts.map((x) => x.id))
    this.selectedKeyId = null
    this.#apply({ layouts: [...this.state.layouts, l], currentId: l.id })
  }
  /** 外部（プリセット・読み込み）から追加する。id が衝突したら付け替える。 */
  add(layout: KeyboardLayout) {
    const l = withUniqueLayoutId(layout, this.state.layouts.map((x) => x.id))
    this.selectedKeyId = null
    this.#apply({ layouts: [...this.state.layouts, l], currentId: l.id })
    return l
  }
  remove() {
    const rest = this.state.layouts.filter((l) => l.id !== this.state.currentId)
    this.selectedKeyId = null
    if (rest.length === 0) {
      const l = newLayout([])
      this.#apply({ layouts: [l], currentId: l.id })
    } else {
      this.#apply({ layouts: rest, currentId: rest[0].id })
    }
  }

  // --- レイアウト属性 ---
  setMeta(patch: Partial<Pick<KeyboardLayout, 'id' | 'name' | 'mode'>>, coalesceKey?: string) {
    const next = { ...this.layout, ...patch }
    this.edit(next, coalesceKey ? `meta:${coalesceKey}` : undefined)
  }

  // --- 行 ---
  addRow(index?: number) {
    this.edit(addRow(this.layout, index))
  }
  removeRow(i: number) {
    this.edit(removeRow(this.layout, i))
  }
  moveRow(from: number, to: number) {
    this.edit(moveRow(this.layout, from, to))
  }
  setRowHeight(i: number, height: number | undefined) {
    this.edit(updateRow(this.layout, i, { height }), `row-height:${i}`)
  }

  // --- キー ---
  addKey(rowIndex: number, kind: KeyKind = 'text') {
    const r = addKey(this.layout, rowIndex, kind)
    if (!r.keyId) return
    this.edit(r.layout)
    this.selectedKeyId = r.keyId
  }
  duplicateKey(id: string) {
    const r = duplicateKey(this.layout, id)
    if (!r.keyId) return
    this.edit(r.layout)
    this.selectedKeyId = r.keyId
  }
  removeKey(id: string) {
    this.edit(removeKey(this.layout, id))
  }
  moveKey(id: string, toRow: number, toIndex: number) {
    this.edit(moveKey(this.layout, id, toRow, toIndex))
  }
  shiftKey(id: string, delta: -1 | 1) {
    this.edit(shiftKey(this.layout, id, delta))
  }
  /** キーを書き換える。`coalesceKey` を渡すと連続した入力を 1 回の undo にまとめる。 */
  updateKey(id: string, key: Key, coalesceKey?: string) {
    this.edit(replaceKey(this.layout, id, key), coalesceKey ? `key:${id}:${coalesceKey}` : undefined)
    if (key.id !== id && this.selectedKeyId === id) this.selectedKeyId = key.id
  }
  changeKind(id: string, kind: KeyKind) {
    const found = findKey(this.layout, id)
    if (found) this.updateKey(id, changeKind(found.key, kind))
  }
  rows(): Row[] {
    return this.layout.rows
  }
  snapshot(): KeyboardLayout {
    return clone(this.layout)
  }
}

export function loadInitialState(fallback: KeyboardLayout[], storage: Pick<Storage, 'getItem'> | undefined): EditorState {
  let saved: EditorState | undefined
  try {
    saved = restoreState(storage?.getItem(STORAGE_KEY) ?? null)
  } catch {
    saved = undefined
  }
  return saved ?? { layouts: fallback.map(clone), currentId: fallback[0].id }
}
