//! 曖昧入力の補正候補（`CandidateSource::Corrected` と `CandidateView.note`）の JSON 形の固定

use nagi_protocol::{CandidateSource, CandidateView, CandidatesView};
use serde_json::json;

#[test]
fn corrected_source_and_note_serialize() {
    let v = CandidateView {
        surface: "日本".into(),
        sources: vec![CandidateSource::Corrected],
        note: Some("にほん".into()),
    };
    assert_eq!(
        serde_json::to_value(&v).unwrap(),
        json!({"surface": "日本", "sources": ["corrected"], "note": "にほん"})
    );
    let back: CandidateView = serde_json::from_value(serde_json::to_value(&v).unwrap()).unwrap();
    assert_eq!(back, v);
}

#[test]
fn note_is_omitted_when_absent_and_optional_on_input() {
    let v = CandidateView {
        surface: "今日".into(),
        sources: vec![CandidateSource::SystemDict],
        note: None,
    };
    // note の無い候補は、従来どおりのキーだけで出力される
    assert_eq!(
        serde_json::to_value(&v).unwrap(),
        json!({"surface": "今日", "sources": ["system_dict"]})
    );
    // note の無い既存の JSON も読める
    let old: CandidatesView = serde_json::from_value(json!({
        "items": [{"surface": "今日", "sources": ["learned"]}],
        "selected": 0, "page_size": 9, "visible": true
    }))
    .unwrap();
    assert_eq!(old.items[0].note, None);
}
