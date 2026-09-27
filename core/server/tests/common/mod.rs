//! テスト間で共有する部品: 小さなインライン辞書と、偽の変換モデル

#![allow(dead_code, missing_docs)]

use nagi_dict::{ConnectionLayout, Dictionary, DictionaryBuilder};
use nagi_infer::{ConversionModel, ConvertRequest, Hypothesis, InferError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const POS_DEFS: &str = "\
0 BOS/EOS,*,*,*,*,*,*
1 名詞,一般,*,*,*,*,*
2 助詞,係助詞,*,*,*,*,*
3 動詞,自立,*,*,五段・カ行促音便,基本形,行く
4 助動詞,*,*,*,特殊・デス,基本形,です
5 記号,句点,*,*,*,*,。
";

pub fn dict() -> Arc<Dictionary> {
    let n = 6;
    let mut cost = vec![100u32; n * n];
    for (rid, lid) in [
        (1, 2),
        (1, 4),
        (2, 3),
        (2, 1),
        (0, 1),
        (0, 3),
        (1, 5),
        (3, 5),
        (4, 5),
        (1, 0),
        (4, 0),
        (5, 0),
        (3, 0),
        (2, 0),
    ] {
        cost[rid * n + lid] = 0;
    }
    let mut lines = vec![n.to_string()];
    lines.extend(cost.iter().map(u32::to_string));
    let mut b = DictionaryBuilder::new();
    b.add_dictionary_text(
        "inline",
        "\
こうえん\t1\t1\t3000\t公園
こうえん\t1\t1\t3500\t講演
こうえん\t1\t1\t4000\t後援
こうえん\t1\t1\t9000\tこうえん
に\t2\t2\t400\tに
いく\t3\t3\t3000\t行く
です\t4\t4\t300\tです
きょう\t1\t1\t3000\t今日
は\t2\t2\t500\tは
",
    )
    .unwrap();
    b.set_pos_defs_text(POS_DEFS).unwrap();
    b.set_connection_text(&lines.join("\n"), ConnectionLayout::RidMajor)
        .unwrap();
    Arc::new(b.build().unwrap())
}

/// 読み → 表記 の表で答え、`score` は表にある表記を最良にする。呼び出し回数と遅延を制御できる
pub struct FakeModel {
    pub table: HashMap<String, String>,
    pub delay: Duration,
    pub converts: Arc<AtomicUsize>,
    pub last_contexts: Arc<Mutex<Vec<String>>>,
}

impl FakeModel {
    pub fn new(pairs: &[(&str, &str)]) -> Self {
        FakeModel {
            table: pairs
                .iter()
                .map(|(r, s)| (r.to_string(), s.to_string()))
                .collect(),
            delay: Duration::ZERO,
            converts: Arc::new(AtomicUsize::new(0)),
            last_contexts: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ConversionModel for FakeModel {
    fn id(&self) -> &str {
        "fake"
    }

    fn convert(&mut self, req: &ConvertRequest) -> Result<Vec<Hypothesis>, InferError> {
        std::thread::sleep(self.delay);
        self.converts.fetch_add(1, Ordering::SeqCst);
        self.last_contexts
            .lock()
            .unwrap()
            .push(req.left_context.clone());
        self.table
            .get(&req.reading)
            .map(|s| {
                vec![Hypothesis {
                    surface: s.clone(),
                    score: 0.0,
                }]
            })
            .ok_or_else(|| InferError::Other("未登録".into()))
    }

    fn score(
        &mut self,
        req: &ConvertRequest,
        candidates: &[String],
    ) -> Result<Vec<f32>, InferError> {
        // 左文脈が「大学の先生が」なら 講演 を、それ以外は 後援 を最良にする（辞書順とは逆）
        let favorite = if req.left_context.contains("先生") {
            "講演"
        } else {
            "後援"
        };
        Ok(candidates
            .iter()
            .map(|c| if c == favorite { -0.1 } else { -5.0 })
            .collect())
    }
}
