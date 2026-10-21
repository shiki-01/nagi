// キャンバスの幾何計算（パン・ズーム・ドロップ位置の判定）。DOM に依存しない純関数。

export interface Rect {
  left: number
  top: number
  right: number
  bottom: number
}

export interface View {
  /** ワールド座標の原点が表示領域の左上から何 px ずれているか。 */
  x: number
  y: number
  scale: number
}

export const SCALE_MIN = 0.2
export const SCALE_MAX = 3

export const clampScale = (s: number) => Math.min(SCALE_MAX, Math.max(SCALE_MIN, s))

/** 表示領域内の点 (cx, cy) を動かさずに倍率を `factor` 倍する。 */
export function zoomAt(view: View, factor: number, cx: number, cy: number): View {
  const scale = clampScale(view.scale * factor)
  const k = scale / view.scale
  return { scale, x: cx - (cx - view.x) * k, y: cy - (cy - view.y) * k }
}

/** 幅 bw × 高さ bh の内容を、表示領域 vw × vh の中央に余白 pad で収める。拡大は maxScale まで。 */
export function fitView(vw: number, vh: number, bw: number, bh: number, pad = 48, maxScale = 1.5): View {
  if (bw <= 0 || bh <= 0 || vw <= 0 || vh <= 0) return { x: 0, y: 0, scale: 1 }
  const scale = clampScale(Math.min(maxScale, Math.max(0.05, (vw - pad * 2) / bw), Math.max(0.05, (vh - pad * 2) / bh)))
  return { scale, x: (vw - bw * scale) / 2, y: (vh - bh * scale) / 2 }
}

export const rectsIntersect = (a: Rect, b: Rect) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top

export function normalizeRect(x1: number, y1: number, x2: number, y2: number): Rect {
  return { left: Math.min(x1, x2), top: Math.min(y1, y2), right: Math.max(x1, x2), bottom: Math.max(y1, y2) }
}

// --- ドロップ位置 ---

export interface RowGeom {
  rect: Rect
  keys: { id: string; rect: Rect }[]
}

export type DropTarget = { kind: 'row'; row: number; index: number } | { kind: 'newRow'; row: number }

const center = (r: Rect) => (r.left + r.right) / 2

/**
 * ポインタ位置 (x, y) から挿入先を決める。行の上端・下端の 25% と行間は「新しい行」、それ以外は
 * その行の中の位置（`exclude` のキーを除いた並びでの挿入位置）。キーボード周辺 `margin` px より外なら null。
 */
export function computeDropTarget(rows: readonly RowGeom[], x: number, y: number, exclude: ReadonlySet<string> = new Set(), margin = 64): DropTarget | null {
  if (rows.length === 0) return null
  const board: Rect = {
    left: Math.min(...rows.map((r) => r.rect.left)),
    right: Math.max(...rows.map((r) => r.rect.right)),
    top: rows[0].rect.top,
    bottom: rows[rows.length - 1].rect.bottom
  }
  if (x < board.left - margin || x > board.right + margin || y < board.top - margin || y > board.bottom + margin) return null

  // 行の境界（隣り合う行の中間）で、ポインタがどの行に属するかを決める。
  let ri = rows.length - 1
  for (let i = 0; i < rows.length - 1; i++) {
    if (y < (rows[i].rect.bottom + rows[i + 1].rect.top) / 2) {
      ri = i
      break
    }
  }
  const rect = rows[ri].rect
  const band = (rect.bottom - rect.top) * 0.25
  if (y < rect.top + band) return { kind: 'newRow', row: ri }
  if (y > rect.bottom - band) return { kind: 'newRow', row: ri + 1 }
  const remaining = rows[ri].keys.filter((k) => !exclude.has(k.id))
  let index = 0
  while (index < remaining.length && center(remaining[index].rect) < x) index++
  return { kind: 'row', row: ri, index }
}

/** ドロップ位置のインジケータ（細い長方形）。`thickness` は同じ座標系での太さ。 */
export function dropIndicator(rows: readonly RowGeom[], target: DropTarget, exclude: ReadonlySet<string> = new Set(), thickness = 4): Rect | null {
  if (rows.length === 0) return null
  const half = thickness / 2
  if (target.kind === 'newRow') {
    const left = Math.min(...rows.map((r) => r.rect.left))
    const right = Math.max(...rows.map((r) => r.rect.right))
    const i = Math.min(Math.max(target.row, 0), rows.length)
    const y = i === 0 ? rows[0].rect.top - 3 : i === rows.length ? rows[rows.length - 1].rect.bottom + 3 : (rows[i - 1].rect.bottom + rows[i].rect.top) / 2
    return { left, right, top: y - half, bottom: y + half }
  }
  const row = rows[target.row]
  if (!row) return null
  const remaining = row.keys.filter((k) => !exclude.has(k.id))
  let x: number
  if (remaining.length === 0) x = center(row.rect)
  else if (target.index < remaining.length) x = remaining[target.index].rect.left - 3
  else x = remaining[remaining.length - 1].rect.right + 3
  return { left: x - half, right: x + half, top: row.rect.top, bottom: row.rect.bottom }
}
