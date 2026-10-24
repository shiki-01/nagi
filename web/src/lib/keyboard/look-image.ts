// 画像ファイルを縮小して data URI にする（ブラウザ専用。canvas を使う）。上限判定・試行順は look-assets.ts の純粋関数。
// 自動テストの対象外（手動確認）。

import { ASSET_LIMITS } from './types'
import { dataUriBytes, encodePlan } from './look-assets'

export type EncodeResult =
  | { ok: true; dataUri: string; bytes: number; width: number; height: number; format: 'image/jpeg' | 'image/png' }
  | { ok: false; error: string }

async function decode(file: Blob): Promise<{ source: CanvasImageSource; width: number; height: number; close: () => void }> {
  if (typeof createImageBitmap === 'function') {
    try {
      const bmp = await createImageBitmap(file)
      return { source: bmp, width: bmp.width, height: bmp.height, close: () => bmp.close() }
    } catch {
      // Image 要素で再試行する
    }
  }
  const url = URL.createObjectURL(file)
  try {
    const img = new Image()
    img.src = url
    await img.decode()
    return { source: img, width: img.naturalWidth, height: img.naturalHeight, close: () => {} }
  } finally {
    URL.revokeObjectURL(url)
  }
}

function toBlob(canvas: HTMLCanvasElement, type: string, quality?: number): Promise<Blob | null> {
  return new Promise((resolve) => canvas.toBlob(resolve, type, quality))
}

function readAsDataURL(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(String(r.result))
    r.onerror = () => reject(r.error)
    r.readAsDataURL(blob)
  })
}

/** 透過ピクセル（alpha < 255）があるか。 */
function hasTransparency(ctx: CanvasRenderingContext2D, w: number, h: number): boolean {
  const data = ctx.getImageData(0, 0, w, h).data
  for (let i = 3; i < data.length; i += 4) if (data[i] < 255) return true
  return false
}

/**
 * ファイルを長辺 1024px 以内に縮小して PNG / JPEG の data URI にする。透過がなければ JPEG、あれば PNG。
 * budget（バイト）に収まらなければ品質→大きさの順に下げて再試行し、それでも収まらなければエラー。
 */
export async function encodeImageFile(file: Blob, budget: number): Promise<EncodeResult> {
  if (budget <= 0) return { ok: false, error: '画像の容量の上限に達しています' }
  let decoded
  try {
    decoded = await decode(file)
  } catch {
    return { ok: false, error: '画像を読み込めませんでした（PNG / JPEG を選んでください）' }
  }
  try {
    if (decoded.width < 1 || decoded.height < 1) return { ok: false, error: '画像を読み込めませんでした' }
    const canvas = document.createElement('canvas')
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return { ok: false, error: 'このブラウザでは画像を変換できません' }
    // 透過の有無は縮小後の大きさで調べる
    const first = encodePlan(decoded.width, decoded.height, false)[0]
    canvas.width = first.width
    canvas.height = first.height
    ctx.drawImage(decoded.source, 0, 0, first.width, first.height)
    const alpha = hasTransparency(ctx, first.width, first.height)
    for (const a of encodePlan(decoded.width, decoded.height, alpha)) {
      canvas.width = a.width
      canvas.height = a.height
      ctx.clearRect(0, 0, a.width, a.height)
      if (a.format === 'image/jpeg') {
        ctx.fillStyle = '#ffffff'
        ctx.fillRect(0, 0, a.width, a.height)
      }
      ctx.drawImage(decoded.source, 0, 0, a.width, a.height)
      const blob = await toBlob(canvas, a.format, a.quality)
      if (!blob || blob.type !== a.format || blob.size > budget) continue
      const dataUri = await readAsDataURL(blob)
      const bytes = dataUriBytes(dataUri)
      if (bytes === undefined || bytes > budget) continue
      return { ok: true, dataUri, bytes, width: a.width, height: a.height, format: a.format }
    }
    return {
      ok: false,
      error: `${Math.round(budget / 1024)} KB（1 枚の上限 ${ASSET_LIMITS.bytesMax / 1024} KB と合計の残りの小さいほう）に収まりませんでした。小さい画像か、単純な絵柄を選んでください`
    }
  } finally {
    decoded.close()
  }
}

/** data URI の画像の元の大きさ（px）。読めなければ undefined。 */
const sizeCache = new Map<string, Promise<{ w: number; h: number } | undefined>>()
export function measureImage(uri: string): Promise<{ w: number; h: number } | undefined> {
  let p = sizeCache.get(uri)
  if (!p) {
    p = new Promise((resolve) => {
      const img = new Image()
      img.onload = () => resolve({ w: img.naturalWidth, h: img.naturalHeight })
      img.onerror = () => resolve(undefined)
      img.src = uri
    })
    if (sizeCache.size > 16) sizeCache.clear()
    sizeCache.set(uri, p)
  }
  return p
}
