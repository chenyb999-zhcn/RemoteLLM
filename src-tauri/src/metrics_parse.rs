use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSample {
    pub name: String,
    pub help: Option<String>,
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

/// 解析 Prometheus 文本格式（仅处理无样本/简单标签的通用场景）
pub fn parse(text: &str) -> Vec<MetricSample> {
    let mut out = Vec::new();
    let mut help: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(h) = line.strip_prefix("# HELP ") {
            let name = h.split_whitespace().next().unwrap_or("").to_string();
            help = Some(h[name.len()..].trim().to_string());
            continue;
        }
        if let Some(t) = line.strip_prefix("# TYPE ") {
            let name = t.split_whitespace().next().unwrap_or("").to_string();
            help = if t.contains(&format!("{name} ")) { help.clone() } else { None };
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        // sample line: name{labels} value [timestamp]
        let (name_part, rest) = match line.find(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == 'N' || c == 'n' || c == 'I') {
            Some(i) => (&line[..i], line[i..].trim()),
            None => continue,
        };
        let (name, labels_raw) = match name_part.rfind('{') {
            Some(i) if name_part.ends_with('}') => {
                (&name_part[..i], &name_part[i + 1..name_part.len() - 1])
            }
            _ => (name_part, ""),
        };
        if name.is_empty() {
            continue;
        }
        let value = match rest.split_whitespace().next() {
            Some(v) => match v.parse::<f64>() {
                Ok(v) => v,
                Err(_) => match v {
                    "+Inf" => f64::INFINITY,
                    "-Inf" => f64::NEG_INFINITY,
                    "NaN" => f64::NAN,
                    _ => continue,
                },
            },
            None => continue,
        };
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
        out.push(MetricSample {
            name: name.to_string(),
            help: help.clone(),
            labels,
            value,
        });
    }
    out
}
