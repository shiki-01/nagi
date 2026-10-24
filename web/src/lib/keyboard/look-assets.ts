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
