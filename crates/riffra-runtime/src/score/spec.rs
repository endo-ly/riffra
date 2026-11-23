use crate::api::output::{ScoreSpec, ScoreSpecTopicSummary};
use crate::api::params::ScoreSpecTopic;

const SCORE_YAML_DOC: &str = include_str!("../../../../docs/score-yaml.md");

struct TopicMeta {
    topic: ScoreSpecTopic,
    slug: &'static str,
    heading: &'static str,
    summary: &'static str,
}

const TOPICS: &[TopicMeta] = &[
    TopicMeta {
        topic: ScoreSpecTopic::Structure,
        slug: "structure",
        heading: "## 1. 全体構造と基本設定",
        summary: "目的と原則、meta（master・fade_out_seconds含む）、form、scale_definitions",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Pitch,
        slug: "pitch",
        heading: "## 2. 音高とスケール",
        summary: "度数記法、オクターブ記号、relative_to、register、cトークン、tonic上書き",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Pattern,
        slug: "pattern",
        heading: "## 3. 時間とパターン",
        summary: "共通時間軸、パターン文法（:長さ・連符・|・||）、旋律・ドラム、奏法記号、スウィング、timing_offset_steps",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Chords,
        slug: "chords",
        heading: "## 4. コードと伴奏",
        summary: "コード記法、修飾子、食い(<)、ボイシング（明示/自動）、comp・arps・counters",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Materials,
        slug: "materials",
        heading: "## 5. 素材の参照・反復・変奏",
        summary: "melodies（from/replace/use/pickup_steps）、sections.from、drums.overrides",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Sections,
        slug: "sections",
        heading: "## 6. セクション構成・時間変更・端数小節",
        summary: "lines、placements、audio_clips、time_changes、last_bar_quarters、拍子単位",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Instruments,
        slug: "instruments",
        heading: "## 7. 楽器・音源・キット",
        summary: "instruments（内蔵・VST3 source・kit・audio・effects）、Lane、kits",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Events,
        slug: "events",
        heading: "## 8. 演奏イベント・マクロ・奏法制御",
        summary: "events[]全種別、macros、優先順位、内蔵音源vsVST3制御境界、source.articulations・cc",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Mix,
        slug: "mix",
        heading: "## 9. ミックスとオートメーション",
        summary: "mix.tracks・buses・sidechain・master・automation",
    },
    TopicMeta {
        topic: ScoreSpecTopic::Diagnostics,
        slug: "diagnostics",
        heading: "## 10. 解析・診断・フィールド契約",
        summary: "YAML構文制約、識別子ルール、診断、未実装拡張、全フィールド契約一覧表",
    },
];

const OVERVIEW_TEMPLATE: &str = r#"# Riffra Score YAML 概要

最小の Score YAML 雛形:

```yaml
meta: { title: Example, bpm: 120, meter: 4/4, tuning: 12edo }
form:
  - { section: verse, bars: 1, tonic: C, scale: ionian }
sections:
  verse:
    lines:
      main: { instrument: lead, melody: theme }
melodies:
  theme: ["1 - - - | 3 - - - | 5 - - - | 1' - - -"]
instruments:
  lead:
    source: { vst3: { plugin: Example Synth } }
```

詳細仕様は `topic` を指定して取得してください（`structure`, `pitch`, `pattern`, `chords`, `materials`, `sections`, `instruments`, `events`, `mix`, `diagnostics`）。
"#;

pub fn get(topic: Option<ScoreSpecTopic>) -> ScoreSpec {
    let available_topics = TOPICS
        .iter()
        .map(|meta| ScoreSpecTopicSummary {
            topic: meta.slug.to_owned(),
            title: meta.heading.trim_start_matches("## ").to_owned(),
            summary: meta.summary.to_owned(),
        })
        .collect();

    let (topic_name, content) = match topic.unwrap_or(ScoreSpecTopic::Overview) {
        ScoreSpecTopic::Overview => ("overview".to_owned(), OVERVIEW_TEMPLATE.to_owned()),
        selected => {
            let index = TOPICS
                .iter()
                .position(|meta| meta.topic == selected)
                .expect("all non-overview ScoreSpecTopic variants are in TOPICS");
            let meta = &TOPICS[index];
            let next_heading = TOPICS.get(index + 1).map(|next| next.heading);
            let section = slice_chapter(meta.heading, next_heading);
            (meta.slug.to_owned(), section.to_owned())
        }
    };

    ScoreSpec {
        topic: topic_name,
        available_topics,
        content,
    }
}

fn slice_chapter<'a>(heading: &str, next_heading: Option<&str>) -> &'a str {
    let start = SCORE_YAML_DOC
        .find(heading)
        .expect("score-yaml.md must contain chapter heading");
    let rest = &SCORE_YAML_DOC[start..];
    match next_heading {
        Some(next) => {
            let needle = format!("\n{next}");
            let end = rest
                .find(&needle)
                .expect("score-yaml.md must contain next chapter heading");
            rest[..end].trim_end()
        }
        None => rest.trim_end(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_lists_all_topics_and_returns_template() {
        let spec = get(None);
        assert_eq!(spec.topic, "overview");
        assert_eq!(spec.available_topics.len(), TOPICS.len());
        assert!(spec.content.contains("meta:"));
    }

    #[test]
    fn every_topic_slices_its_chapter_cleanly() {
        for meta in TOPICS {
            let spec = get(Some(meta.topic));
            assert_eq!(spec.topic, meta.slug);
            assert!(
                spec.content.starts_with(meta.heading),
                "topic {} did not start with {}",
                meta.slug,
                meta.heading
            );
            assert!(!spec.content.is_empty());
        }
    }
}
