//! 最小限の GGUF リーダー。語彙（`tokenizer.ggml.*`）だけを取り出す。
//!
//! zenz は「1 文字 = 1 トークン」の文字レベルモデルで、llama.cpp のトークナイザ／デトークナイザを
//! 経由すると文字が化ける（BPE のバイトエスケープ規則が適用される）ため、語彙の生の文字列を
//! GGUF から直接読み、トークン化・デコードを自前で行う。

use crate::InferError;
use std::collections::HashMap;
use std::io::{BufReader, Read};

/// GGUF から読み出した語彙情報
#[derive(Debug, Clone, Default)]
pub struct GgufVocab {
    /// トークン ID → 生の文字列
    pub tokens: Vec<String>,
    /// トークン ID → 種別（llama.cpp の `LLAMA_TOKEN_TYPE_*`。無ければ空）
    pub token_types: Vec<i32>,
    /// `tokenizer.ggml.eos_token_id`
    pub eos_id: Option<u32>,
    /// `tokenizer.ggml.bos_token_id`
    pub bos_id: Option<u32>,
    /// `tokenizer.ggml.unknown_token_id`
    pub unk_id: Option<u32>,
    /// `tokenizer.ggml.merges`（`"a b"` 形式の BPE マージ規則。順序が優先度）
    pub merges: Vec<String>,
    /// `tokenizer.ggml.pre`
    pub pre: Option<String>,
    /// `tokenizer.ggml.model`
    pub model: Option<String>,
}

impl GgufVocab {
    /// 1 文字（デコード後）だけから成るトークンの「文字 → ID」表を作る（同じ文字が複数あれば最小の ID を優先）
    pub fn char_map(&self) -> HashMap<char, u32> {
        let mut map = HashMap::new();
        for (id, bytes) in self.token_bytes().iter().enumerate() {
            if let Ok(s) = std::str::from_utf8(bytes) {
                let mut it = s.chars();
                if let (Some(c), None) = (it.next(), it.next()) {
                    map.entry(c).or_insert(id as u32);
                }
            }
        }
        map
    }

    /// 1 バイトだけから成るトークンの「バイト → ID」表（語彙に無い文字のフォールバック用）
    pub fn byte_map(&self) -> HashMap<u8, u32> {
        let mut map = HashMap::new();
        for (id, bytes) in self.token_bytes().iter().enumerate() {
            if let [b] = bytes.as_slice() {
                map.entry(*b).or_insert(id as u32);
            }
        }
        map
    }
}

fn io_err(e: std::io::Error) -> InferError {
    InferError::ModelLoad(format!("GGUF 読み込み失敗: {e}"))
}

struct Reader<R: Read> {
    inner: R,
}

impl<R: Read> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], InferError> {
        let mut buf = [0u8; N];
        self.inner.read_exact(&mut buf).map_err(io_err)?;
        Ok(buf)
    }
    fn u32(&mut self) -> Result<u32, InferError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, InferError> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> Result<String, InferError> {
        let len = self.u64()? as usize;
        if len > 64 * 1024 * 1024 {
            return Err(InferError::ModelLoad("GGUF: 文字列が大きすぎます".into()));
        }
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf).map_err(io_err)?;
        String::from_utf8(buf).map_err(|_| InferError::ModelLoad("GGUF: UTF-8 不正".into()))
    }
    fn skip(&mut self, n: u64) -> Result<(), InferError> {
        std::io::copy(&mut (&mut self.inner).take(n), &mut std::io::sink()).map_err(io_err)?;
        Ok(())
    }
    /// スカラー型のバイト幅（文字列・配列は None）
    fn scalar_size(ty: u32) -> Option<u64> {
        match ty {
            0 | 1 | 7 => Some(1),
            2 | 3 => Some(2),
            4..=6 => Some(4),
            10..=12 => Some(8),
            _ => None,
        }
    }
    fn skip_value(&mut self, ty: u32) -> Result<(), InferError> {
        if let Some(size) = Self::scalar_size(ty) {
            return self.skip(size);
        }
        match ty {
            8 => {
                let len = self.u64()?;
                self.skip(len)
            }
            9 => {
                let elem = self.u32()?;
                let len = self.u64()?;
                if let Some(size) = Self::scalar_size(elem) {
                    self.skip(size * len)
                } else {
                    for _ in 0..len {
                        self.skip_value(elem)?;
                    }
                    Ok(())
                }
            }
            _ => Err(InferError::ModelLoad(format!("GGUF: 未知の値型 {ty}"))),
        }
    }
    fn read_u32_like(&mut self, ty: u32) -> Result<Option<i64>, InferError> {
        Ok(match ty {
            2 => Some(u16::from_le_bytes(self.bytes()?) as i64),
            3 => Some(i16::from_le_bytes(self.bytes()?) as i64),
            4 => Some(u32::from_le_bytes(self.bytes()?) as i64),
            5 => Some(i32::from_le_bytes(self.bytes()?) as i64),
            _ => {
                self.skip_value(ty)?;
                None
            }
        })
    }
}

/// GGUF ファイルのメタデータから語彙を読み出す
pub fn read_vocab(path: &std::path::Path) -> Result<GgufVocab, InferError> {
    let file = std::fs::File::open(path).map_err(io_err)?;
    let mut r = Reader {
        inner: BufReader::with_capacity(1 << 20, file),
    };
    if &r.bytes::<4>()? != b"GGUF" {
        return Err(InferError::ModelLoad("GGUF: マジック不正".into()));
    }
    let version = r.u32()?;
    if version < 2 {
        return Err(InferError::ModelLoad(format!(
            "GGUF: 未対応バージョン {version}"
        )));
    }
    let _tensor_count = r.u64()?;
    let kv_count = r.u64()?;
    let mut vocab = GgufVocab::default();
    for _ in 0..kv_count {
        let key = r.string()?;
        let ty = r.u32()?;
        match key.as_str() {
            "tokenizer.ggml.tokens" if ty == 9 => {
                let elem = r.u32()?;
                let len = r.u64()?;
                if elem != 8 {
                    return Err(InferError::ModelLoad(
                        "GGUF: tokens が文字列配列でない".into(),
                    ));
                }
                vocab.tokens = (0..len).map(|_| r.string()).collect::<Result<_, _>>()?;
            }
            "tokenizer.ggml.token_type" if ty == 9 => {
                let elem = r.u32()?;
                let len = r.u64()?;
                let mut types = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    types.push(r.read_u32_like(elem)?.unwrap_or(0) as i32);
                }
                vocab.token_types = types;
            }
            "tokenizer.ggml.merges" if ty == 9 => {
                let elem = r.u32()?;
                let len = r.u64()?;
                if elem != 8 {
                    return Err(InferError::ModelLoad(
                        "GGUF: merges が文字列配列でない".into(),
                    ));
                }
                vocab.merges = (0..len).map(|_| r.string()).collect::<Result<_, _>>()?;
            }
            "tokenizer.ggml.eos_token_id" => vocab.eos_id = r.read_u32_like(ty)?.map(|v| v as u32),
            "tokenizer.ggml.bos_token_id" => vocab.bos_id = r.read_u32_like(ty)?.map(|v| v as u32),
            "tokenizer.ggml.unknown_token_id" => {
                vocab.unk_id = r.read_u32_like(ty)?.map(|v| v as u32)
            }
            "tokenizer.ggml.pre" if ty == 8 => vocab.pre = Some(r.string()?),
            "tokenizer.ggml.model" if ty == 8 => vocab.model = Some(r.string()?),
            _ => r.skip_value(ty)?,
        }
    }
    Ok(vocab)
}

/// GPT-2 の「バイト → 表示文字」表の逆引き（表示文字 → 元のバイト）
///
/// 印字可能な Latin-1 のバイトはそのまま、それ以外は U+0100 から順に割り当てられる。
pub fn gpt2_byte_decoder() -> HashMap<char, u8> {
    let mut map = HashMap::new();
    let mut extra = 0u32;
    for b in 0..=255u32 {
        let printable =
            (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b);
        if printable {
            map.insert(char::from_u32(b).unwrap_or('\u{fffd}'), b as u8);
        } else {
            map.insert(char::from_u32(256 + extra).unwrap_or('\u{fffd}'), b as u8);
            extra += 1;
        }
    }
    map
}

impl GgufVocab {
    /// 各トークンの実際のバイト列（GPT-2 のバイトエスケープを戻したもの）
    pub fn token_bytes(&self) -> Vec<Vec<u8>> {
        let decoder = gpt2_byte_decoder();
        self.tokens
            .iter()
            .map(|tok| {
                let mut bytes = Vec::new();
                for c in tok.chars() {
                    match decoder.get(&c) {
                        Some(&b) => bytes.push(b),
                        None => {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                }
                bytes
            })
            .collect()
    }
}

/// zenz（`gpt2-small-japanese-char`）用のトークナイザ
///
/// 1 文字ずつに分割し、各文字の UTF-8 バイト列を GPT-2 のバイトエスケープ表記にして、
/// GGUF の merges で BPE を適用する（llama.cpp の対応外の pre-tokenizer を自前で再現する）。
#[derive(Debug, Clone)]
pub struct CharBpeTokenizer {
    ids: HashMap<String, u32>,
    ranks: HashMap<(String, String), usize>,
    byte_encoder: Vec<char>,
    token_bytes: Vec<Vec<u8>>,
    unk_id: u32,
}

impl CharBpeTokenizer {
    /// 語彙から構築する
    pub fn new(vocab: &GgufVocab) -> Self {
        let decoder = gpt2_byte_decoder();
        let mut byte_encoder = vec!['\0'; 256];
        for (c, b) in decoder {
            byte_encoder[b as usize] = c;
        }
        let ids = vocab
            .tokens
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i as u32))
            .collect();
        let ranks = vocab
            .merges
            .iter()
            .enumerate()
            .filter_map(|(rank, m)| {
                let (a, b) = m.split_once(' ')?;
                Some(((a.to_string(), b.to_string()), rank))
            })
            .collect();
        Self {
            ids,
            ranks,
            byte_encoder,
            token_bytes: vocab.token_bytes(),
            unk_id: vocab.unk_id.unwrap_or(0),
        }
    }

    /// 文字列をトークン ID 列にする（BOS/EOS は付けない）
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4];
        for c in text.chars() {
            let mut parts: Vec<String> = c
                .encode_utf8(&mut buf)
                .bytes()
                .map(|b| self.byte_encoder[b as usize].to_string())
                .collect();
            loop {
                let best = (0..parts.len().saturating_sub(1))
                    .filter_map(|i| {
                        self.ranks
                            .get(&(parts[i].clone(), parts[i + 1].clone()))
                            .map(|&r| (r, i))
                    })
                    .min();
                let Some((_, i)) = best else { break };
                let merged = format!("{}{}", parts[i], parts[i + 1]);
                parts.splice(i..=i + 1, [merged]);
            }
            out.extend(
                parts
                    .iter()
                    .map(|p| self.ids.get(p).copied().unwrap_or(self.unk_id)),
            );
        }
        out
    }

    /// トークン ID のバイト列（範囲外は空）
    pub fn token_bytes(&self, id: u32) -> &[u8] {
        self.token_bytes.get(id as usize).map_or(&[], Vec::as_slice)
    }

    /// トークン ID 列を文字列に戻す（不正な UTF-8 は置換文字）
    pub fn decode(&self, ids: &[u32]) -> String {
        let bytes: Vec<u8> = ids
            .iter()
            .flat_map(|&i| self.token_bytes(i).iter().copied())
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn model_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/models/zenz-v3.1-small-Q5_K_M.gguf")
    }

    #[test]
    fn dump_zenz_vocab() {
        let path = model_path();
        if !path.exists() {
            eprintln!("skip: モデル未取得");
            return;
        }
        let v = read_vocab(&path).unwrap();
        eprintln!(
            "model={:?} pre={:?} n_tokens={} eos={:?} bos={:?} unk={:?} types={}",
            v.model,
            v.pre,
            v.tokens.len(),
            v.eos_id,
            v.bos_id,
            v.unk_id,
            v.token_types.len()
        );
        for id in (0..16).chain(v.tokens.len().saturating_sub(16)..v.tokens.len()) {
            eprintln!("{id}: {:?} type={:?}", v.tokens[id], v.token_types.get(id));
        }
        let map = v.char_map();
        for c in ['\u{EE00}', '\u{EE01}', '\u{EE02}', 'キ', 'き', '今', 'は'] {
            eprintln!("{c:?} -> {:?}", map.get(&c));
        }
        eprintln!("char_map size {}", map.len());
        let tok = CharBpeTokenizer::new(&v);
        let prompt = "\u{EE00}キョウハイイテンキデス\u{EE01}";
        let ids = tok.encode(prompt);
        eprintln!("prompt ids {:?}", ids);
        assert_eq!(tok.decode(&ids), prompt);
        eprintln!("merges {}", v.merges.len());
    }
}
