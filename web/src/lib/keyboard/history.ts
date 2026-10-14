// undo/redo 用の履歴。状態は不変値として扱う（呼び出し側が破壊的変更をしない）。

export class History<T> {
  #past: T[] = []
  #future: T[] = []
  #lastKey: string | undefined
  #lastAt = 0
  readonly limit: number
  readonly coalesceMs: number

  constructor(
    private present: T,
    options: { limit?: number; coalesceMs?: number } = {}
  ) {
    this.limit = options.limit ?? 200
    this.coalesceMs = options.coalesceMs ?? 1000
  }

  get value(): T {
    return this.present
  }
  get canUndo(): boolean {
    return this.#past.length > 0
  }
  get canRedo(): boolean {
    return this.#future.length > 0
  }

  /**
   * 新しい状態を積む。`coalesceKey` が直前と同じで `coalesceMs` 以内なら、
   * 1 つの undo 単位にまとめる（テキスト入力の 1 文字ごとに履歴を作らないため）。
   */
  push(next: T, coalesceKey?: string, now = Date.now()): void {
    if (Object.is(next, this.present)) return
    const merge = coalesceKey !== undefined && coalesceKey === this.#lastKey && now - this.#lastAt <= this.coalesceMs
    if (!merge) {
      this.#past.push(this.present)
      if (this.#past.length > this.limit) this.#past.shift()
    }
    this.present = next
    this.#future = []
    this.#lastKey = coalesceKey
    this.#lastAt = now
  }

  undo(): T {
    const prev = this.#past.pop()
    if (prev !== undefined) {
      this.#future.push(this.present)
      this.present = prev
    }
    this.#lastKey = undefined
    return this.present
  }

  redo(): T {
    const next = this.#future.pop()
    if (next !== undefined) {
      this.#past.push(this.present)
      this.present = next
    }
    this.#lastKey = undefined
    return this.present
  }
}
