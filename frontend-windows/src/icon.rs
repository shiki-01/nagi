//! 入力方式アイコン（.ico）の生成（OS 非依存）。
//!
//! `ITfInputProcessorProfileMgr::RegisterProfile` はアイコンを「.ico / .exe / .dll のパスと番号」で受ける。
//! リソースの埋め込みは追加のツール（rc / windres）に依存するため、アイコンはコードで描いて .ico として書き出す
//! （decisions.md H-41）。絵柄は藍色の角丸の四角に白い「N」。32bpp（BGRA）の DIB 形式で、16/24/32/48 px を含む。

/// 含める大きさ（px）
pub const ICON_SIZES: [u32; 4] = [16, 24, 32, 48];

/// 背景の色（RGB）
const BACKGROUND: [u8; 3] = [0x2B, 0x5C, 0x8A];
/// 「N」の色（RGB）
const FOREGROUND: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// 超サンプリングの 1 辺の分割数
const SAMPLES: u32 = 4;

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (p.0 - a.0, p.1 - a.1);
    let len2 = abx * abx + aby * aby;
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + abx * t, a.1 + aby * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// 単位正方形 [0,1]² 上の点が角丸の四角の内側か
fn in_rounded_square(p: (f32, f32)) -> bool {
    const R: f32 = 0.22;
    let cx = p.0.clamp(R, 1.0 - R);
    let cy = p.1.clamp(R, 1.0 - R);
    (p.0 - cx).powi(2) + (p.1 - cy).powi(2) <= R * R
}

/// 単位正方形上の点が「N」の線の上か
fn in_letter(p: (f32, f32)) -> bool {
    const HALF: f32 = 0.065;
    let (top, bottom, left, right) = (0.27, 0.73, 0.30, 0.70);
    [
        ((left, top), (left, bottom)),
        ((right, top), (right, bottom)),
        ((left, top), (right, bottom)),
    ]
    .iter()
    .any(|(a, b)| dist_to_segment(p, *a, *b) <= HALF)
}

/// `size` px 四方の RGBA（行は上から下、非乗算）。端はアンチエイリアスされる
pub fn render_rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let n = SAMPLES * SAMPLES;
    for y in 0..size {
        for x in 0..size {
            let (mut shape, mut letter) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32,
                        (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32,
                    );
                    if in_rounded_square(p) {
                        shape += 1;
                        if in_letter(p) {
                            letter += 1;
                        }
                    }
                }
            }
            // 図形の被覆率 = alpha。図形の内側のうち文字の割合で前景と背景を混ぜる
            let alpha = (shape * 255 + n / 2) / n;
            let mix = if shape == 0 {
                0.0
            } else {
                letter as f32 / shape as f32
            };
            for c in 0..3 {
                let v = f32::from(BACKGROUND[c]) * (1.0 - mix) + f32::from(FOREGROUND[c]) * mix;
                out.push(v.round() as u8);
            }
            out.push(alpha as u8);
        }
    }
    out
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// 1 枚分の DIB（BITMAPINFOHEADER + 下から上の BGRA + AND マスク）
fn dib(size: u32) -> Vec<u8> {
    let rgba = render_rgba(size);
    let mut out = Vec::new();
    push_u32(&mut out, 40);
    push_u32(&mut out, size);
    push_u32(&mut out, size * 2); // 高さは XOR + AND の 2 倍
    push_u16(&mut out, 1);
    push_u16(&mut out, 32);
    push_u32(&mut out, 0); // BI_RGB
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    for row in (0..size).rev() {
        for col in 0..size {
            let i = ((row * size + col) * 4) as usize;
            out.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
        }
    }
    // AND マスク（1bpp、各行 4 バイト境界）。透明度は alpha で表すので 0
    let mask_row = size.div_ceil(32) * 4;
    out.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    out
}

/// `.ico` ファイルの内容
pub fn encode_ico() -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES.iter().map(|&s| (s, dib(s))).collect();
    let mut out = Vec::new();
    push_u16(&mut out, 0);
    push_u16(&mut out, 1); // ICO
    push_u16(&mut out, images.len() as u16);
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, data) in &images {
        out.push(*size as u8);
        out.push(*size as u8);
        out.push(0); // パレット無し
        out.push(0);
        push_u16(&mut out, 1); // planes
        push_u16(&mut out, 32);
        push_u32(&mut out, data.len() as u32);
        push_u32(&mut out, offset);
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        out.extend_from_slice(data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    #[test]
    fn ico_directory_is_consistent() {
        let ico = encode_ico();
        assert_eq!(u16_at(&ico, 0), 0);
        assert_eq!(u16_at(&ico, 2), 1);
        let count = usize::from(u16_at(&ico, 4));
        assert_eq!(count, ICON_SIZES.len());
        for (i, expected_size) in ICON_SIZES.iter().enumerate() {
            let e = 6 + 16 * i;
            let size = u32::from(ico[e]);
            assert_eq!(size, *expected_size);
            assert_eq!(u16_at(&ico, e + 6), 32, "bit count");
            let len = u32_at(&ico, e + 8) as usize;
            let off = u32_at(&ico, e + 12) as usize;
            assert!(off + len <= ico.len());
            // DIB ヘッダ
            assert_eq!(u32_at(&ico, off), 40);
            assert_eq!(u32_at(&ico, off + 4), size);
            assert_eq!(u32_at(&ico, off + 8), size * 2);
            let expected =
                40 + (size * size * 4) as usize + (size.div_ceil(32) * 4 * size) as usize;
            assert_eq!(len, expected);
        }
        assert_eq!(
            ico.len(),
            6 + 16 * count
                + (0..count)
                    .map(|i| u32_at(&ico, 6 + 16 * i + 8) as usize)
                    .sum::<usize>()
        );
    }

    #[test]
    fn corners_are_transparent_and_letter_is_light() {
        let size = 48;
        let px = render_rgba(size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "corner is outside the rounded square");
        // 左の縦線の中央（x = 0.30, y = 0.5）は白
        assert_eq!(at(14, 24), [0xFF, 0xFF, 0xFF, 0xFF]);
        // 文字の外側の内部は背景色で不透明
        assert_eq!(at(24, 4), [0x2B, 0x5C, 0x8A, 0xFF]);
    }
}
