// /custom-keyboard の編集状態。localStorage に自動保存し、undo/redo を持つ。

import { History } from './history'
import {
  addKey,
  addKeyInNewRow,
  addRow,
  changeKind,
  duplicateKey,
  duplicateLayout,
  findKey,
  insertKey,
  moveKey,
  moveKeysChecked,
  nudgeKeys,
  removeKeys,
  allKeyIds,
  moveRow,
  newLayout,
  removeRow,
  replaceKey,
  shiftKey,
  updateRow,
  withUniqueLayoutId,
  clone
} from './ops'
import { KEY_KINDS, LIMITS } from './types'
import type { Direction, MoveDest } from './ops'
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
  #selected = $state.raw<string[]>([])
  /** 操作できなかった理由など、短く見せる通知。 */
  notice = $state<string | null>(null)
  saveError = $state<string | null>(null)
  /** 履歴の変化をリアクティブにするためのカウンタ。 */
  #rev = $state(0)

  constructor(initial: EditorState) {
    this.#history = new History(initial)
    this.state = initial
  }

  /** 選択中のキー id（複数選択可）。 */
  get selectedIds(): string[] {
    return this.#selected
  }
  set selectedIds(ids: string[]) {
    this.#selected = [...new Set(ids)]
  }
  /** 選択がちょうど 1 つのときの id（インスペクタで詳細を編集する対象）。 */
  get selectedKeyId(): string | null {
    return this.#selected.length === 1 ? this.#selected[0] : null
  }
  set selectedKeyId(id: string | null) {
    this.#selected = id === null ? [] : [id]
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
    const alive = new Set(allKeyIds(this.layout))
    if (this.#selected.some((id) => !alive.has(id))) this.#selected = this.#selected.filter((id) => alive.has(id))
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
    this.edit(removeKeys(this.layout, [id]))
  }
  moveKey(id: string, toRow: number, toIndex: number) {
    this.edit(moveKey(this.layout, id, toRow, toIndex))
  }
  /** 複数のキーを移動する。選択は維持。上限を超えるときは通知を出して何もしない。 */
  moveKeys(ids: readonly string[], dest: MoveDest): boolean {
    const r = moveKeysChecked(this.layout, ids, dest)
    if (r.error) {
      this.notice = r.error
      return false
    }
    if (r.layout === this.layout) return false
    this.notice = null
    this.edit(r.layout)
    return true
  }
  /** キーを追加して選択する。`dest` は moveKeys と同じ指す位置（index は行内の挿入位置）。 */
  addKeyAt(kind: KeyKind, dest: MoveDest): boolean {
    const r = 'row' in dest ? insertKey(this.layout, dest.row, dest.index, kind) : addKeyInNewRow(this.layout, dest.newRow, kind)
    if (!r.keyId) {
      this.notice = 'row' in dest ? `1 行に置けるキーは最大 ${LIMITS.keysPerRowMax} 個までです` : `行は最大 ${LIMITS.rowsMax} 行までです`
      return false
    }
    this.notice = null
    this.edit(r.layout)
    this.selectedIds = [r.keyId]
    return true
  }
  removeSelected() {
    if (this.#selected.length === 0) return
    this.notice = null
    this.edit(removeKeys(this.layout, this.#selected))
  }
  nudgeSelected(dir: Direction) {
    if (this.#selected.length === 0) return
    this.edit(nudgeKeys(this.layout, this.#selected, dir))
  }
  selectAll() {
    this.#selected = allKeyIds(this.layout)
  }
  shiftKey(id: string, delta: -1 | 1) {
    this.edit(shiftKey(this.layout, id, delta))
  }
  /** キーを書き換える。`coalesceKey` を渡すと連続した入力を 1 回の undo にまとめる。 */
  updateKey(id: string, key: Key, coalesceKey?: string) {
    this.edit(replaceKey(this.layout, id, key), coalesceKey ? `key:${id}:${coalesceKey}` : undefined)
    if (key.id !== id && this.#selected.includes(id)) this.#selected = this.#selected.map((x) => (x === id ? key.id : x))
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
