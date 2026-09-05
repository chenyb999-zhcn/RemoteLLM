use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSample {
    pub name: String,
    pub help: Option<String>,
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

/// 解析 Prometheus 文本格式（支持标签、+Inf/-Inf/NaN、可选毫秒时间戳）
pub fn parse(text: &str) -> Vec<MetricSample> {
    let mut out = Vec::new();
    let mut help: Option<(String, String)> = None; // (指标名, 帮助文本)
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("# HELP ") {
            let (name, rest) = h.split_once(' ').unwrap_or((h, ""));
            help = Some((name.to_string(), rest.trim().to_string()));
            continue;
        }
        if line.starts_with('#') {
            // # TYPE 等其余注释行忽略
            continue;
        }
        // 样本行: name{labels} value [timestamp]
        let (name_part, value_str) = match split_name_value(line) {
            Some(x) => x,
            None => continue,
        };
        let value: f64 = match value_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let (name, labels_raw) = match name_part.rfind('{') {
            Some(i) if name_part.ends_with('}') => {
                (&name_part[..i], &name_part[i + 1..name_part.len() - 1])
            }
            _ => (name_part, ""),
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let labels: Vec<(String, String)> = labels_raw
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                Some((
                    k.trim().to_string(),
                    v.trim().trim_matches('"').to_string(),
                ))
            })
            .collect();
        let hlp = help
            .as_ref()
            .filter(|(n, _)| n == name)
            .map(|(_, t)| t.clone());
        out.push(MetricSample {
            name: name.to_string(),
            help: hlp,
            labels,
            value,
        });
    }
    out
}

/// 把样本行拆成 (name{labels} 部分, 值)；末尾 13+ 位纯数字视为毫秒时间戳，值取倒数第二字段
fn split_name_value(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_end();
    let sp = t.rfind(' ')?;
    let (head, last) = (&t[..sp], &t[sp + 1..]);
    if is_ms_timestamp(last) {
        let sp2 = head.rfind(' ')?;
        let cand = &head[sp2 + 1..];
        if is_ms_timestamp(cand) {
            return None;
        }
        Some((&head[..sp2], cand))
    } else {
        Some((head, last))
    }
}

fn is_ms_timestamp(s: &str) -> bool {
    s.len() >= 13 && s.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    // llama.cpp llama-server --metrics 暴露的全部指标（与上游 tools/server/tests/unit/test_metrics.py 一致）
    const LLAMA_METRICS: &[&str] = &[
        "llamacpp:prompt_tokens_total",
        "llamacpp:prompt_tokens_cached_total",
        "llamacpp:prompt_seconds_total",
        "llamacpp:tokens_predicted_total",
        "llamacpp:tokens_predicted_seconds_total",
        "llamacpp:n_decode_total",
        "llamacpp:n_tokens_max",
        "llamacpp:spec_decode_num_draft_tokens_total",
        "llamacpp:spec_decode_num_accepted_tokens_total",
        "llamacpp:spec_decode_num_drafts_total",
        "llamacpp:prompt_tokens_seconds",
        "llamacpp:predicted_tokens_seconds",
        "llamacpp:requests_processing",
        "llamacpp:requests_deferred",
        "llamacpp:n_busy_slots_per_decode",
    ];

    #[test]
    fn parses_all_llamacpp_metrics() {
        let mut text = String::new();
        for (i, name) in LLAMA_METRICS.iter().enumerate() {
            text.push_str(&format!(
                "# HELP {name} test help\n# TYPE {name} counter\n{name} {}\n",
                i as u64
            ));
        }
        let samples = parse(&text);
        assert_eq!(samples.len(), LLAMA_METRICS.len(), "所有指标都应被解析");
        for (i, name) in LLAMA_METRICS.iter().enumerate() {
            let s = samples.iter().find(|s| s.name == *name).unwrap();
            assert_eq!(s.value, i as f64);
            assert_eq!(s.help.as_deref(), Some("test help"));
            assert!(s.labels.is_empty());
        }
    }

    #[test]
    fn parses_metric_with_labels() {
        let text = "# HELP vllm:num_requests_running Number of running requests.\n\
                    # TYPE vllm:num_requests_running gauge\n\
                    vllm:num_requests_running{model_name=\"Qwen/Qwen3-8B\", gpu_model=\"V100\"} 2\n";
        let samples = parse(text);
        assert_eq!(samples.len(), 1);
        let s = &samples[0];
        assert_eq!(s.name, "vllm:num_requests_running");
        assert_eq!(s.value, 2.0);
        assert_eq!(
            s.labels,
            vec![
                ("model_name".into(), "Qwen/Qwen3-8B".into()),
                ("gpu_model".into(), "V100".into()),
            ]
        );
        assert_eq!(s.help.as_deref(), Some("Number of running requests."));
    }

    #[test]
    fn parses_special_values() {
        let text = "a:pos_inf +Inf\na:neg_inf -Inf\na:nan NaN\n";
        let samples = parse(text);
        assert_eq!(samples.len(), 3);
        assert!(samples[0].value.is_infinite() && samples[0].value > 0.0);
        assert!(samples[1].value.is_infinite() && samples[1].value < 0.0);
        assert!(samples[2].value.is_nan());
    }

    #[test]
    fn parses_line_with_millisecond_timestamp() {
        let text = "foo:bar 5 1720000000000\n";
        let samples = parse(text);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "foo:bar");
        assert_eq!(samples[0].value, 5.0);
    }

    #[test]
    fn parses_name_without_letters_n() {
        // 回归：旧解析器按"行内第一个数字/n/N"定位值，会误伤名称含 n/N/I 的指标
        let text = "llamacpp:requests_deferred 0\nllamacpp:tokens_predicted_total 42\n";
        let samples = parse(text);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].name, "llamacpp:requests_deferred");
        assert_eq!(samples[0].value, 0.0);
        assert_eq!(samples[1].name, "llamacpp:tokens_predicted_total");
        assert_eq!(samples[1].value, 42.0);
    }

    #[test]
    fn skips_invalid_lines() {
        let text = "# comment\nnot a sample line at all\n\n# HELP x:z help\nx:z\n";
        let samples = parse(text);
        assert!(samples.is_empty());
    }
}
