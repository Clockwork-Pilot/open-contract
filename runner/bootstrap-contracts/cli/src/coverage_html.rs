use super::ContractError;
use runner_batch::{BatchReport, CoverageLineFile, CoverageRecord};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const METRICS: [&str; 6] = [
    "line",
    "statement",
    "function",
    "branch",
    "region",
    "instantiation",
];

pub(super) fn write_report(
    output: &Path,
    source_root: &Path,
    report: &BatchReport,
) -> Result<(), ContractError> {
    fs::create_dir_all(output).map_err(|error| {
        ContractError::from((
            2,
            format!("cannot create HTML coverage directory {}: {error}", output.display()),
        ))
    })?;
    for (index, contract) in report.coverage.iter().enumerate() {
        let filename = contract_filename(index);
        let html = contract_page(contract, index, source_root);
        fs::write(output.join(filename), html).map_err(|error| {
            ContractError::from((
                2,
                format!("cannot write HTML coverage report for {}: {error}", contract.contract),
            ))
        })?;
    }
    fs::write(output.join("index.html"), index_page(&report.coverage)).map_err(|error| {
        ContractError::from((
            2,
            format!("cannot write HTML coverage index {}: {error}", output.display()),
        ))
    })
}

fn index_page(contracts: &[CoverageRecord]) -> String {
    let mut body = String::from("<h1>Open Contract coverage</h1><p>Coverage is reported from the language adapter artifacts. Dataset entries link to their contract's coverage union.</p><table><thead><tr><th>Contract</th><th>Declared dataset</th><th>Line coverage</th><th>Report</th></tr></thead><tbody>");
    for (index, contract) in contracts.iter().enumerate() {
        let line = contract.aggregate_metrics.get("line");
        let line_summary = metric_summary(line.map(|metric| metric.status.as_str()), line.and_then(|metric| metric.covered), line.and_then(|metric| metric.total), line.and_then(|metric| metric.percent));
        for (dataset_index, dataset) in contract.datasets.iter().enumerate() {
            body.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td><a href=\"{}#dataset-{}\">Open entity report</a></td></tr>",
                escape(&contract.contract),
                escape(dataset),
                line_summary,
                contract_filename(index),
                dataset_index,
            ));
        }
    }
    body.push_str("</tbody></table>");
    document("Open Contract coverage", body)
}

fn contract_page(contract: &CoverageRecord, index: usize, source_root: &Path) -> String {
    let mut body = format!(
        "<nav><a href=\"index.html\">Coverage index</a></nav><h1>{}</h1><p class=\"note\">Entity data below is the coverage union for this contract across its declared datasets.</p><h2 id=\"summary\">Metric summary</h2><div class=\"cards\">",
        escape(&contract.contract)
    );
    for metric in METRICS {
        let outcome = contract.aggregate_metrics.get(metric);
        let status = outcome.map(|value| value.status.as_str());
        let summary = metric_summary(
            status,
            outcome.and_then(|value| value.covered),
            outcome.and_then(|value| value.total),
            outcome.and_then(|value| value.percent),
        );
        let entity_count = contract.coverage_entities.iter().filter(|entity| {
            entity.get("metric").and_then(Value::as_str) == Some(metric)
        }).count();
        let location_note = if status == Some("available") && entity_count == 0 {
            "<small>Totals available; individual locations unavailable from adapter data.</small>"
        } else {
            ""
        };
        body.push_str(&format!(
            "<section class=\"card\"><h3>{}</h3><p>{}</p>{}</section>",
            escape(metric),
            summary,
            location_note,
        ));
    }
    body.push_str("</div><h2>Declared datasets</h2><ul>");
    for (dataset_index, dataset) in contract.datasets.iter().enumerate() {
        body.push_str(&format!(
            "<li id=\"dataset-{dataset_index}\">{} <span class=\"muted\">contract coverage union</span></li>",
            escape(dataset)
        ));
    }
    body.push_str("</ul><h2 id=\"entities\">Source entities</h2>");

    let line_files = merge_lines(&contract.line_coverage, &contract.additional_line_coverage);
    let mut paths = line_files.keys().cloned().collect::<BTreeSet<_>>();
    paths.extend(contract.coverage_entities.iter().filter_map(|entity| {
        entity.get("path").and_then(Value::as_str).map(str::to_owned)
    }));
    if paths.is_empty() {
        body.push_str("<p class=\"muted\">The adapters supplied no source locations for this contract.</p>");
    }
    for path in paths {
        body.push_str(&render_source_file(&path, &line_files, &contract.coverage_entities, source_root));
    }
    body.push_str(&format!("<footer>Contract report {}</footer>", index + 1));
    document(&format!("{} coverage", contract.contract), body)
}

fn render_source_file(
    path: &str,
    line_files: &BTreeMap<String, (BTreeSet<u32>, BTreeSet<u32>)>,
    entities: &[Value],
    source_root: &Path,
) -> String {
    let file_entities = entities
        .iter()
        .filter(|entity| entity.get("path").and_then(Value::as_str) == Some(path))
        .collect::<Vec<_>>();
    let mut html = format!("<section class=\"file\"><h3>{}</h3>", escape(path));
    let source = source_contents(source_root, path);
    let (executable, covered) = line_files.get(path).cloned().unwrap_or_default();
    if !executable.is_empty() {
        html.push_str("<h4>Lines</h4><table><thead><tr><th>Line</th><th>Status</th><th>Count</th><th>Source</th></tr></thead><tbody>");
        for line in executable {
            let status = if covered.contains(&line) { "covered" } else { "uncovered" };
            let content = source
                .as_ref()
                .and_then(|lines| lines.get(line.saturating_sub(1) as usize))
                .map(String::as_str)
                .unwrap_or("source text unavailable");
            html.push_str(&format!(
                "<tr class=\"{}\"><td>{}</td><td>{}</td><td>Not reported by adapter</td><td><code>{}</code></td></tr>",
                status, line, status, escape(content)
            ));
        }
        html.push_str("</tbody></table>");
    }
    html.push_str("<h4>Adapter entities</h4>");
    if file_entities.is_empty() {
        html.push_str("<p class=\"muted\">No per-entity locations were supplied for this source. Metric totals remain available above where supported.</p>");
    } else {
        html.push_str("<table><thead><tr><th>Metric</th><th>Entity</th><th>Location</th><th>Status</th><th>Count</th></tr></thead><tbody>");
        for entity in file_entities {
            let metric = entity.get("metric").and_then(Value::as_str).unwrap_or("unknown");
            let name = entity.get("name").and_then(Value::as_str).unwrap_or("(unnamed)");
            let status = entity.get("status").and_then(Value::as_str).unwrap_or("unavailable");
            let location = location_text(entity.get("location").unwrap_or(&Value::Null));
            let count = entity.get("count").and_then(Value::as_u64)
                .map(|value| value.to_string()).unwrap_or_else(|| "Not reported".into());
            html.push_str(&format!(
                "<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(status), escape(metric), escape(name), escape(&location), escape(status), count
            ));
        }
        html.push_str("</tbody></table>");
    }
    html.push_str("</section>");
    html
}

fn location_text(location: &Value) -> String {
    if location.get("precision").and_then(Value::as_str) == Some("unavailable") {
        return "Location unavailable".into();
    }
    let start = location.get("start_line").and_then(Value::as_u64);
    let end = location.get("end_line").and_then(Value::as_u64);
    let Some(start) = start else {
        return "Location unavailable".into();
    };
    let mut value = match end.filter(|end| *end != start) {
        Some(end) => format!("{start}–{end}"),
        None => start.to_string(),
    };
    if let Some(column) = location.get("start_column").and_then(Value::as_u64) {
        value.push_str(&format!(":{}", column + 1));
    }
    value
}

fn merge_lines(
    dataset: &[CoverageLineFile],
    supplemental: &[CoverageLineFile],
) -> BTreeMap<String, (BTreeSet<u32>, BTreeSet<u32>)> {
    let mut files = BTreeMap::new();
    for file in dataset.iter().chain(supplemental) {
        let (executable, covered) = files.entry(file.path.clone()).or_insert_with(|| (BTreeSet::new(), BTreeSet::new()));
        executable.extend(file.executable_lines.iter().copied());
        covered.extend(file.covered_lines.iter().copied());
    }
    files
}

fn source_contents(root: &Path, path: &str) -> Option<Vec<String>> {
    // The runner validates adapter paths against the manifest's package-relative source whitelist.
    fs::read_to_string(root.join(path)).ok().map(|source| source.lines().map(str::to_owned).collect())
}

fn metric_summary(status: Option<&str>, covered: Option<u64>, total: Option<u64>, percent: Option<f64>) -> String {
    match (status, covered, total, percent) {
        (Some("available"), Some(covered), Some(total), Some(percent)) => {
            format!("{covered}/{total} · {percent:.1}%")
        }
        _ => "Unavailable: adapter does not report this metric or its totals".into(),
    }
}

fn contract_filename(index: usize) -> String {
    format!("contract-{index:03}.html")
}

fn document(title: &str, body: String) -> String {
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{}</style></head><body><main>{}</main></body></html>", escape(title), STYLE, body)
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const STYLE: &str = "body{font:14px system-ui,sans-serif;margin:0;background:#f6f8fa;color:#24292f}main{max-width:1200px;margin:2rem auto;padding:0 1.5rem}a{color:#0969da}.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:.75rem}.card,.file{background:white;border:1px solid #d0d7de;border-radius:6px;padding:1rem;margin:1rem 0}.card h3{margin:.1rem 0 .6rem;text-transform:capitalize}table{border-collapse:collapse;width:100%;background:white;margin:1rem 0}th,td{border:1px solid #d8dee4;padding:.45rem;text-align:left;vertical-align:top}th{background:#f6f8fa}.covered{background:#dafbe1}.uncovered{background:#ffebe9}.unavailable{background:#fff8c5}.muted,.note,footer{color:#57606a}.note{padding:.75rem;background:#fff8c5;border-left:4px solid #bf8700}code{white-space:pre-wrap;font-family:ui-monospace,monospace}footer{margin:2rem 0}nav{margin-bottom:1rem}";

#[cfg(test)]
mod tests {
    use super::write_report;
    use runner_batch::{BatchReport, CoverageLineFile, CoverageMetricOutcome, CoverageRecord};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    fn metrics(available: &[&str]) -> BTreeMap<String, CoverageMetricOutcome> {
        ["line", "statement", "function", "branch", "region", "instantiation"]
            .into_iter()
            .map(|name| {
                let is_available = available.contains(&name);
                (
                    name.to_owned(),
                    CoverageMetricOutcome {
                        status: if is_available { "available" } else { "unavailable" }.into(),
                        covered: is_available.then_some(1),
                        total: is_available.then_some(2),
                        percent: is_available.then_some(50.0),
                        minimum_percent: Some(80.0),
                        threshold_met: is_available.then_some(false),
                        files: Vec::new(),
                    },
                )
            })
            .collect()
    }

    fn entity(path: &str, metric: &str, name: &str, line: u64, status: &str, count: Option<u64>) -> Value {
        json!({
            "path": path,
            "metric": metric,
            "name": name,
            "location": {
                "precision": "line",
                "start_line": line,
                "start_column": null,
                "end_line": line,
                "end_column": null,
            },
            "status": status,
            "count": count,
        })
    }

    fn contract(name: &str, dataset: &str, source: &str, native: Value, available: &[&str], entities: Vec<Value>) -> CoverageRecord {
        CoverageRecord {
            contract: name.into(),
            dataset_paths: Vec::new(),
            datasets: vec![dataset.into()],
            minimum_coverage_percent: 80.0,
            dataset_covered_lines: 1,
            dataset_executable_lines: 2,
            combined_covered_lines: 1,
            combined_executable_lines: 2,
            line_threshold_met: false,
            coverage_threshold_met: false,
            dataset_metrics: metrics(available),
            aggregate_metrics: metrics(available),
            coverage_entities: entities,
            native_report: native,
            declared_sources: vec![source.into()],
            covered_sources: vec![source.into()],
            missing_sources: Vec::new(),
            line_coverage: vec![CoverageLineFile {
                path: source.into(),
                executable_lines: vec![1, 2],
                covered_lines: vec![1],
            }],
            additional_test_coverage: Value::Null,
            additional_line_coverage: Vec::new(),
        }
    }

    #[test]
    fn renders_js_python_and_rust_adapter_entities_without_inference() {
        let root = tempfile::tempdir().unwrap();
        for (path, source) in [
            ("js/validator.js", "export function validate() {\n  return true;\n}\n"),
            ("py/validator.py", "def validate(value):\n    if value:\n        return True\n    return False\n"),
            ("rust/src/lib.rs", "pub fn validate(value: bool) -> bool {\n    value\n}\n"),
        ] {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, source).unwrap();
        }

        let report = BatchReport {
            results: Vec::new(),
            coverage: vec![
                contract(
                    "javascript",
                    "dataset.yml",
                    "js/validator.js",
                    json!({"format":"v8-inspector","sources":["js/validator.js"],"result":[]}),
                    &["line", "statement", "function", "branch"],
                    vec![
                        entity("js/validator.js", "line", "line 1", 1, "covered", None),
                        entity("js/validator.js", "function", "validate", 1, "covered", Some(2)),
                        entity("js/validator.js", "branch", "branch 1.2", 2, "uncovered", Some(0)),
                    ],
                ),
                contract(
                    "python",
                    "nested/dataset.yml",
                    "py/validator.py",
                    json!({"format":"coverage.py","sources":["py/validator.py"],"files":{"py/validator.py":{"lines":[1,2],"arcs":[[1,2],[2,3]]}}}),
                    &["line", "statement", "branch"],
                    vec![
                        entity("py/validator.py", "branch", "branch 2 → 3", 2, "covered", None),
                        entity("py/validator.py", "branch", "branch 2 → 4", 2, "uncovered", None),
                    ],
                ),
                contract(
                    "rust",
                    "dataset.json",
                    "rust/src/lib.rs",
                    json!({"format":"llvm-cov-export","report":{"data":[{"files":[],"functions":[]}]}}),
                    &["line", "function", "region", "instantiation"],
                    vec![
                        entity("rust/src/lib.rs", "function", "validate", 1, "covered", Some(3)),
                        entity("rust/src/lib.rs", "region", "validate region 2", 2, "uncovered", Some(0)),
                    ],
                ),
            ],
        };
        let output = root.path().join("html");
        write_report(&output, root.path(), &report).unwrap();

        let index = std::fs::read_to_string(output.join("index.html")).unwrap();
        assert!(index.contains("dataset.yml"));
        assert!(index.contains("nested/dataset.yml"));
        assert!(index.contains("contract-002.html#dataset-0"));

        let javascript = std::fs::read_to_string(output.join("contract-000.html")).unwrap();
        assert!(javascript.contains("branch 1.2"));
        assert!(javascript.contains("uncovered"));
        assert!(javascript.contains("1/2 · 50.0%"));

        let python = std::fs::read_to_string(output.join("contract-001.html")).unwrap();
        assert!(python.contains("Unavailable: adapter does not report this metric"));
        assert!(python.contains("branch 2 → 4"));
        assert!(python.contains("Not reported"));

        let rust = std::fs::read_to_string(output.join("contract-002.html")).unwrap();
        assert!(rust.contains("Totals available; individual locations unavailable from adapter data."));
        assert!(rust.contains("validate region 2"));
        assert!(rust.contains("uncovered"));
    }
}
