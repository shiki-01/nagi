// 背景画像（assets）の形式・大きさの判定（docs/keyboard-style-spec.md §5.5、§6.2）。
// ブラウザ依存（canvas での縮小・data URI 化）は含めず、サイズ計算・上限判定の純粋関数だけを置く。

import { ASSET_LIMITS } from './types'

const DATA_URI = /^data:(image\/png|image\/jpeg);base64,([A-Za-z0-9+/]+)(={0,2})$/

/** data URI（PNG / JPEG の base64）のデコード後のバイト数。形式違い・空・base64 不正なら undefined。 */
export function dataUriBytes(uri: unknown): number | undefined {
  if (typeof uri !== 'string') return undefined
  const m = DATA_URI.exec(uri)
  if (!m) return undefined
  const length = m[2].length + m[3].length
  if (length % 4 !== 0) return undefined
  return (length / 4) * 3 - m[3].length
}

/** assets 全体の使用量（バイト）。数えられないもの（不正な値）は 0 として扱う。 */
export function assetsUsage(assets: Record<string, unknown>): number {
  return Object.values(assets).reduce<number>((sum, uri) => sum + (dataUriBytes(uri) ?? 0), 0)
}

/** 1 枚追加できるか（個数・1 枚・合計の上限）。できなければ理由。 */
export function canAddAsset(assets: Record<string, unknown>, bytes: number): string | undefined {
  if (Object.keys(assets).length >= ASSET_LIMITS.countMax) return `画像は ${ASSET_LIMITS.countMax} 枚までです`
  if (bytes > ASSET_LIMITS.bytesMax) return `1 枚は ${ASSET_LIMITS.bytesMax / 1024} KB 以内にしてください（${Math.ceil(bytes / 1024)} KB）`
  if (assetsUsage(assets) + bytes > ASSET_LIMITS.totalMax)
    return `画像の合計は ${ASSET_LIMITS.totalMax / 1024} KB 以内にしてください`
  return undefined
}

// ---- エディタが画像を追加するときの計算（純粋関数。canvas を使う部分は look-image.ts） -------------------------

/** 長辺が max 以内になるよう縮小した大きさ（縮小のみ。1px 未満にはしない）。 */
export function fitWithin(width: number, height: number, max: number): { width: number; height: number } {
  const long = Math.max(width, height)
  const k = long > max ? max / long : 1
  return { width: Math.max(1, Math.round(width * k)), height: Math.max(1, Math.round(height * k)) }
}

export interface EncodeAttempt {
  format: 'image/jpeg' | 'image/png'
  /** JPEG のみ（0〜1）。 */
  quality?: number
  width: number
  height: number
}

/** JPEG の品質を下げていく順。 */
export const JPEG_QUALITIES = [0.85, 0.75, 0.65, 0.55, 0.45] as const
/** 品質を下げても収まらないときの縮小率（JPEG は品質 0.7 固定、PNG はこの率のみ）。 */
export const SHRINK_STEPS = [0.85, 0.7, 0.55, 0.4, 0.3, 0.2] as const

/**
 * 画像を data URI にするときに試す順序（§8.1）。長辺 1024px 以内に縮小し、透過がなければ JPEG（品質 0.85 から下げる）、
 * あれば PNG。それでも上限を超えるなら JPEG は縮小、PNG は縮小を強める。呼び出し側は最初に上限へ収まったものを採用する。
 */
export function encodePlan(width: number, height: number, hasAlpha: boolean): EncodeAttempt[] {
  const base = fitWithin(width, height, ASSET_LIMITS.longSideMax)
  const scaled = (k: number) => ({ width: Math.max(1, Math.round(base.width * k)), height: Math.max(1, Math.round(base.height * k)) })
  if (hasAlpha) return [{ format: 'image/png', ...base }, ...SHRINK_STEPS.map((k) => ({ format: 'image/png' as const, ...scaled(k) }))]
  return [
    ...JPEG_QUALITIES.map((quality) => ({ format: 'image/jpeg' as const, quality, ...base })),
    ...SHRINK_STEPS.map((k) => ({ format: 'image/jpeg' as const, quality: 0.7, ...scaled(k) }))
  ]
}

/** 次に追加する 1 枚が使えるバイト数（1 枚の上限と、合計の残りの小さいほう）。 */
export function assetByteBudget(assets: Record<string, unknown>): number {
  return Math.max(0, Math.min(ASSET_LIMITS.bytesMax, ASSET_LIMITS.totalMax - assetsUsage(assets)))
}

/** ファイル名から画像 id（ASSET_ID_PATTERN）を作る。used と衝突したら -2, -3 … を付ける。 */
export function assetIdFromName(filename: string, used: Iterable<string>): string {
  const taken = new Set(used)
  let base = filename
    .replace(/\.[^.]*$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/^[^a-z0-9]+/, '')
    .replace(/-+$/, '')
    .slice(0, 24)
  if (base === '') base = 'image'
  if (!taken.has(base)) return base
  for (let n = 2; ; n++) {
    const cand = `${base}-${n}`
    if (!taken.has(cand)) return cand
  }
}

/** バイト数の表示（例: 12.3 KB）。 */
export function formatKB(bytes: number): string {
  return `${(bytes / 1024).toFixed(bytes >= 10 * 1024 ? 0 : 1)} KB`
}

/** PNG / JPEG の data URI から画像の大きさ（px）を読む（ヘッダーのみ。壊れていれば undefined）。 */
export function imageDimensions(uri: unknown): { width: number; height: number } | undefined {
  if (dataUriBytes(uri) === undefined) return undefined
  let b: Uint8Array
  try {
    const bin = atob((uri as string).slice((uri as string).indexOf(',') + 1))
    b = Uint8Array.from(bin, (ch) => ch.charCodeAt(0))
  } catch {
    return undefined
  }
  if (b.length > 24 && b[0] === 0x89 && b[1] === 0x50) {
    const u32 = (i: number) => ((b[i] << 24) | (b[i + 1] << 16) | (b[i + 2] << 8) | b[i + 3]) >>> 0
    return { width: u32(16), height: u32(20) }
  }
  if (b[0] === 0xff && b[1] === 0xd8) {
    let i = 2
    while (i + 9 < b.length) {
      if (b[i] !== 0xff) return undefined
      const m = b[i + 1]
      if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc)
        return { height: (b[i + 5] << 8) | b[i + 6], width: (b[i + 7] << 8) | b[i + 8] }
      i += 2 + ((b[i + 2] << 8) | b[i + 3])
    }
  }
  return undefined
}
