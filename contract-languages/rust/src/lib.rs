//! Rust implementation of the Open Contract language batch protocol.
//!
//! The runner owns isolation, dataset flattening, expected-value comparison,
//! and predicates. A Rust contract calls [`run_dataset`] from its own command
//! and supplies the validator function it wants invoked for every request.

use serde_json::{json, Value};
#[cfg(coverage)]
use std::path::{Path, PathBuf};

/// LLVM profile snapshots shared with Rust bootstrap dataset coverage.
#[cfg(coverage)]
pub mod coverage_snapshot;

/// Invoke a Rust validator for each flattened request and return the adapter
/// protocol object expected by the Open Contract runner. The callback receives
/// the real input and a context containing case/step identity and optional
/// setup metadata. Parallel steps in one adjacent group run concurrently.
pub fn run_dataset(
    request: &Value,
    validate: impl Fn(&Value, &Value) -> Result<Value, String> + Sync,
) -> Result<Value, String> {
    let requests = request
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| "batch request must contain a requests array".to_string())?;
    let invocation_root = std::env::current_dir()
        .map_err(|error| format!("cannot read contract working directory: {error}"))?;
    let package = request
        .get("package")
        .and_then(Value::as_str)
        .unwrap_or(".");
    let package_root = invocation_root
        .join(package)
        .canonicalize()
        .map_err(|error| format!("cannot resolve contract package {package:?}: {error}"))?;
    if !package_root.starts_with(&invocation_root) {
        return Err("contract package path escaped the staged root".into());
    }
    let coverage_sources: Vec<String> = request
        .get("coverage_sources")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let mut results = Vec::with_capacity(requests.len());
    let collect_fingerprints = request
        .get("coverage_fingerprints")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    #[cfg(coverage)]
    let mut snapshots = Vec::with_capacity(if collect_fingerprints {
        requests.len()
    } else {
        1
    });
    for item in requests {
        item.get("key")
            .ok_or_else(|| "batch request entry is missing key".to_string())?;
        item.get("input")
            .ok_or_else(|| "batch request entry is missing input".to_string())?;
    }
    let mut index = 0;
    while index < requests.len() {
        let first = &requests[index];
        let case_id = first["key"]["case_id"]
            .as_str()
            .ok_or_else(|| "batch request key is missing case_id".to_string())?;
        let mut end = index + 1;
        if first["parallel"].as_bool() == Some(true) {
            while end < requests.len()
                && requests[end]["key"]["case_id"].as_str() == Some(case_id)
                && requests[end]["parallel"].as_bool() == Some(true)
            {
                end += 1;
            }
        }
        #[cfg(coverage)]
        if collect_fingerprints {
            unsafe {
                coverage_snapshot::reset_counters();
            }
        }
        let group_results = if end - index > 1 {
            std::thread::scope(|scope| {
                let handles = requests[index..end]
                    .iter()
                    .map(|item| scope.spawn(|| invoke_validator(&validate, item)))
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle
                            .join()
                            .map_err(|_| "parallel validator call panicked".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })?
        } else {
            vec![invoke_validator(&validate, first)]
        };
        #[cfg(coverage)]
        if collect_fingerprints {
            let snapshot = coverage_snapshot::snapshot()?;
            for item in &requests[index..end] {
                snapshots.push((
                    serde_json::to_string(&item["key"]).map_err(|error| error.to_string())?,
                    snapshot.clone(),
                ));
            }
        }
        results.extend(group_results);
        index = end;
    }

    #[cfg(coverage)]
    if !collect_fingerprints {
        snapshots.push(("aggregate".to_string(), coverage_snapshot::snapshot()?));
    }
    #[cfg(coverage)]
    let coverage = llvm_coverage::report(
        &snapshots,
        &package_root,
        &coverage_sources,
        collect_fingerprints,
    )?;
    #[cfg(not(coverage))]
    let _ = (&package_root, &coverage_sources, collect_fingerprints);
    #[cfg(not(coverage))]
    let coverage = unavailable_coverage(&coverage_sources);

    Ok(json!({
        "results": results,
        "coverage": coverage.native_report,
        "coverage_scopes": coverage.scopes,
        "coverage_lines": coverage.lines,
    }))
}

fn invoke_validator(
    validate: &(impl Fn(&Value, &Value) -> Result<Value, String> + Sync),
    item: &Value,
) -> Value {
    let context = json!({
        "case_id": item["key"]["case_id"],
        "step_id": item["key"]["step_id"],
        "setup": item["setup"],
        "parallel": item["parallel"].as_bool().unwrap_or(false),
    });
    match validate(&item["input"], &context) {
        Ok(output) => json!({ "key": item["key"], "output": output }),
        Err(error) => json!({ "key": item["key"], "error": error }),
    }
}

#[cfg(not(coverage))]
fn unavailable_coverage(sources: &[String]) -> CoverageReport {
    let _ = sources;
    CoverageReport {
        native_report: json!({
            "format": "llvm-instrprof",
            "sources": [],
            "error": "Rust adapter was not compiled with -Cinstrument-coverage and --cfg coverage"
        }),
        scopes: Vec::new(),
        lines: Vec::new(),
    }
}

struct CoverageReport {
    native_report: Value,
    scopes: Vec<Value>,
    lines: Vec<Value>,
}

#[cfg(coverage)]
mod llvm_coverage {
    use super::{CoverageReport, Path, PathBuf, Value};
    use serde_json::json;
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    pub(super) fn report(
        snapshots: &[(String, Vec<u8>)],
        package_root: &Path,
        declared_sources: &[String],
        collect_fingerprints: bool,
    ) -> Result<CoverageReport, String> {
        if snapshots.is_empty() {
            return Err("Rust adapter received an empty request batch".into());
        }
        let temp = TempDirectory::create()?;
        let binary = std::env::current_exe()
            .map_err(|error| format!("cannot resolve Rust contract executable: {error}"))?;
        let llvm_profdata = tool("LLVM_PROFDATA", "llvm-profdata", package_root);
        let llvm_cov = tool("LLVM_COV", "llvm-cov", package_root);
        let source_paths = declared_sources
            .iter()
            .map(|source| {
                let path = package_root.join(source);
                if !path.starts_with(package_root) || !path.is_file() {
                    return Err(format!(
                        "declared Rust coverage source is unavailable: {source}"
                    ));
                }
                Ok(path)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if source_paths.is_empty() {
            return Err("Rust adapter requires declared coverage sources".into());
        }

        let mut coverage_scopes = Vec::with_capacity(if collect_fingerprints {
            snapshots.len()
        } else {
            0
        });
        if collect_fingerprints {
            for (index, (key, bytes)) in snapshots.iter().enumerate() {
                let profile = temp.path.join(format!("case-{index}.profraw"));
                fs::write(&profile, bytes)
                    .map_err(|error| format!("cannot write Rust case profile: {error}"))?;
                let report = export(
                    &llvm_profdata,
                    &llvm_cov,
                    &profile,
                    &binary,
                    &source_paths,
                    &temp.path,
                    index,
                )?;
                let files = coverage_files(&report, package_root, declared_sources);
                let key: Value = serde_json::from_str(key)
                    .map_err(|error| format!("cannot decode saved request key: {error}"))?;
                coverage_scopes.push(json!({ "key": key, "files": files }));
            }
        }

        let aggregate = temp.path.join("aggregate.profraw");
        let aggregate_bytes = if collect_fingerprints {
            super::coverage_snapshot::restore_aggregate(snapshots)?;
            super::coverage_snapshot::snapshot()?
        } else {
            snapshots[0].1.clone()
        };
        fs::write(&aggregate, aggregate_bytes)
            .map_err(|error| format!("cannot write aggregate Rust profile: {error}"))?;
        let mut aggregate_report = export(
            &llvm_profdata,
            &llvm_cov,
            &aggregate,
            &binary,
            &source_paths,
            &temp.path,
            snapshots.len(),
        )?;
        filter_export_to_declared_sources(&mut aggregate_report, package_root, declared_sources);
        let lines = coverage_line_files(&aggregate_report, package_root, declared_sources);
        let sources = lines
            .iter()
            .filter_map(|item| item.get("path").and_then(Value::as_str))
            .collect::<Vec<_>>();
        Ok(CoverageReport {
            native_report: json!({ "format": "llvm-cov-export", "sources": sources, "report": aggregate_report }),
            scopes: coverage_scopes,
            lines,
        })
    }

    fn tool(variable: &str, fallback: &str, package_root: &Path) -> PathBuf {
        let path = std::env::var_os(variable)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(fallback));
        if path.is_absolute() {
            path
        } else {
            package_root.join(path)
        }
    }

    fn export(
        llvm_profdata: &Path,
        llvm_cov: &Path,
        profile: &Path,
        binary: &Path,
        sources: &[PathBuf],
        temporary: &Path,
        index: usize,
    ) -> Result<Value, String> {
        let indexed = temporary.join(format!("profile-{index}.profdata"));
        let merge = Command::new(llvm_profdata)
            .args(["merge", "-sparse"])
            .arg(profile)
            .arg("-o")
            .arg(&indexed)
            .output()
            .map_err(|error| {
                format!(
                    "cannot run llvm-profdata at {} (set LLVM_PROFDATA): {error}",
                    llvm_profdata.display()
                )
            })?;
        if !merge.status.success() {
            return Err(format!(
                "llvm-profdata failed: {}",
                String::from_utf8_lossy(&merge.stderr).trim()
            ));
        }
        let mut command = Command::new(llvm_cov);
        command
            .arg("export")
            .arg(format!("-instr-profile={}", indexed.display()))
            .arg(binary);
        command.args(sources);
        let output = command.output().map_err(|error| {
            format!(
                "cannot run llvm-cov at {} (set LLVM_COV): {error}",
                llvm_cov.display()
            )
        })?;
        if !output.status.success() {
            return Err(format!(
                "llvm-cov export failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("llvm-cov returned invalid JSON: {error}"))
    }

    fn files(report: &Value) -> impl Iterator<Item = &Value> {
        report
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(|data| {
                data.get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
    }

    fn filter_export_to_declared_sources(
        report: &mut Value,
        package_root: &Path,
        declared: &[String],
    ) {
        let Some(data) = report.get_mut("data").and_then(Value::as_array_mut) else {
            return;
        };
        for item in data {
            let (public_filenames, filtered_totals) = {
                let Some(files) = item.get_mut("files").and_then(Value::as_array_mut) else {
                    continue;
                };
                files.retain(|file| {
                    file.get("filename")
                        .and_then(Value::as_str)
                        .and_then(|filename| package_relative(filename, package_root, declared))
                        .is_some_and(|path| declared.iter().any(|source| source == &path))
                });
                let filenames = files
                    .iter()
                    .filter_map(|file| file.get("filename").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect::<std::collections::HashSet<_>>();
                (filenames, summarize_files(files))
            };
            if let Some(functions) = item.get_mut("functions").and_then(Value::as_array_mut) {
                functions.retain(|function| {
                    function
                        .get("filenames")
                        .and_then(Value::as_array)
                        .is_some_and(|filenames| {
                            !filenames.is_empty()
                                && filenames.iter().all(|filename| {
                                    filename
                                        .as_str()
                                        .is_some_and(|filename| public_filenames.contains(filename))
                                })
                        })
                });
            }
            if let Some(totals) = item.get_mut("totals") {
                *totals = filtered_totals;
            }
        }
    }

    fn summarize_files(files: &[Value]) -> Value {
        let mut totals = serde_json::Map::new();
        let categories = files
            .iter()
            .filter_map(|file| file.get("summary").and_then(Value::as_object))
            .flat_map(|summary| summary.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        for category in categories {
            let mut count = 0_u64;
            let mut covered = 0_u64;
            for summary in files
                .iter()
                .filter_map(|file| file.get("summary").and_then(|s| s.get(&category)))
            {
                count += summary.get("count").and_then(Value::as_u64).unwrap_or(0);
                covered += summary.get("covered").and_then(Value::as_u64).unwrap_or(0);
            }
            let percent = if count == 0 {
                100.0
            } else {
                covered as f64 * 100.0 / count as f64
            };
            totals.insert(
                category,
                json!({ "count": count, "covered": covered, "percent": percent }),
            );
        }
        Value::Object(totals)
    }

    fn package_relative(
        filename: &str,
        package_root: &Path,
        declared: &[String],
    ) -> Option<String> {
        let candidate = Path::new(filename);
        if let Ok(relative) = candidate.strip_prefix(package_root) {
            return Some(relative.to_string_lossy().replace('\\', "/"));
        }
        declared
            .iter()
            .find(|source| {
                candidate.canonicalize().ok().as_ref()
                    == package_root.join(source).canonicalize().ok().as_ref()
            })
            .cloned()
    }

    fn covered_lines(file: &Value) -> Vec<u32> {
        let mut lines = std::collections::BTreeSet::new();
        for segment in file
            .get("segments")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(values) = segment.as_array() else {
                continue;
            };
            if values.len() < 4 {
                continue;
            }
            let Some(line) = values[0].as_u64().and_then(|n| u32::try_from(n).ok()) else {
                continue;
            };
            let count = values[2].as_u64().unwrap_or(0);
            let has_count = values[3].as_bool().unwrap_or(false);
            if count > 0 && has_count {
                lines.insert(line);
            }
        }
        lines.into_iter().collect()
    }

    fn coverage_files(report: &Value, root: &Path, declared: &[String]) -> Vec<Value> {
        let mut result = files(report)
            .filter_map(|file| {
                let filename = file.get("filename")?.as_str()?;
                let path = package_relative(filename, root, declared)?;
                if !declared.iter().any(|source| {
                    path == *source
                        || path.starts_with(&format!("{}/", source.trim_end_matches('/')))
                }) {
                    return None;
                }
                let lines = covered_lines(file);
                (!lines.is_empty()).then(|| json!({ "path": path, "lines": lines }))
            })
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        result
    }

    fn coverage_line_files(report: &Value, root: &Path, declared: &[String]) -> Vec<Value> {
        let mut result = files(report)
            .filter_map(|file| {
                let filename = file.get("filename")?.as_str()?;
                let path = package_relative(filename, root, declared)?;
                if !declared.iter().any(|source| {
                    path == *source
                        || path.starts_with(&format!("{}/", source.trim_end_matches('/')))
                }) {
                    return None;
                }
                let mut line_counts = std::collections::BTreeMap::<u32, Vec<u64>>::new();
                for segment in file
                    .get("segments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(values) = segment.as_array() else {
                        continue;
                    };
                    if values.len() < 4 {
                        continue;
                    }
                    let Some(line) = values[0].as_u64().and_then(|n| u32::try_from(n).ok()) else {
                        continue;
                    };
                    if values[3].as_bool().unwrap_or(false) {
                        line_counts
                            .entry(line)
                            .or_default()
                            .push(values[2].as_u64().unwrap_or(0));
                    }
                }
                let executable = line_counts.keys().copied().collect::<Vec<_>>();
                // LLVM emits multiple executable segments for a line when it has
                // multiple regions. Count that source line as covered only when
                // all executable regions on it ran; otherwise line coverage is
                // overstated compared with llvm-cov's own summary.
                let covered = line_counts
                    .iter()
                    .filter_map(|(line, counts)| {
                        counts.iter().all(|count| *count > 0).then_some(*line)
                    })
                    .collect::<Vec<_>>();
                Some(json!({
                    "path": path,
                    "executable_lines": executable,
                    "covered_lines": covered
                }))
            })
            .collect::<Vec<_>>();
        result.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        result
    }

    struct TempDirectory {
        path: PathBuf,
    }
    impl TempDirectory {
        fn create() -> Result<Self, String> {
            let base = std::env::temp_dir();
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!(
                "open-contract-rust-coverage-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).map_err(|error| {
                format!(
                    "cannot create Rust coverage scratch directory {}: {error}",
                    path.display()
                )
            })?;
            Ok(Self { path })
        }
    }
    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::filter_export_to_declared_sources;
        use serde_json::json;
        use std::fs;

        #[test]
        fn llvm_native_report_contains_only_declared_sources_and_totals() {
            let root = std::env::temp_dir().join(format!(
                "open-contract-coverage-filter-test-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/public.rs"), "pub fn api() {}\n").unwrap();
            fs::write(root.join("src/helper.rs"), "fn helper() {}\n").unwrap();
            let public = root.join("src/public.rs").to_string_lossy().into_owned();
            let private = root.join("src/helper.rs").to_string_lossy().into_owned();
            let mut report = json!({"data": [{
                "files": [
                    {"filename": public, "summary": {"lines": {"count": 2, "covered": 1, "percent": 50.0}}},
                    {"filename": private, "summary": {"lines": {"count": 20, "covered": 0, "percent": 0.0}}}
                ],
                "functions": [
                    {"name": "api", "filenames": [root.join("src/public.rs").to_string_lossy()]},
                    {"name": "helper", "filenames": [root.join("src/helper.rs").to_string_lossy()]}
                ],
                "totals": {"lines": {"count": 22, "covered": 1, "percent": 4.5}}
            }]});

            filter_export_to_declared_sources(&mut report, &root, &["src/public.rs".into()]);

            let data = &report["data"][0];
            assert_eq!(data["files"].as_array().unwrap().len(), 1);
            assert_eq!(
                data["files"][0]["filename"],
                root.join("src/public.rs").to_string_lossy().as_ref()
            );
            assert_eq!(data["functions"].as_array().unwrap().len(), 1);
            assert_eq!(data["totals"]["lines"]["count"], 2);
            assert_eq!(data["totals"]["lines"]["covered"], 1);

            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_dataset;
    use serde_json::{json, Value};

    #[test]
    fn returns_normalized_coverage_line_records() {
        let request = json!({
            "coverage_sources": ["src/lib.rs"],
            "requests": [{"key":{"case_id":"one","step_id":null},"input":{"value":1}}]
        });
        let report = run_dataset(&request, |input, _| Ok(input.clone())).unwrap();
        let lines = report["coverage_lines"]
            .as_array()
            .expect("coverage_lines is an array");
        #[cfg(coverage)]
        assert!(
            !lines.is_empty(),
            "instrumented Rust coverage must include the declared source"
        );
        #[cfg(not(coverage))]
        assert_eq!(lines, &Vec::<Value>::new());
        for file in lines {
            assert!(file["path"].as_str().is_some_and(|path| !path.is_empty()));
            #[cfg(coverage)]
            assert_eq!(file["path"], "src/lib.rs");
            let executable = file["executable_lines"]
                .as_array()
                .expect("executable_lines is an array");
            let covered = file["covered_lines"]
                .as_array()
                .expect("covered_lines is an array");
            assert!(executable
                .windows(2)
                .all(|pair| pair[0].as_u64() < pair[1].as_u64()));
            assert!(covered
                .windows(2)
                .all(|pair| pair[0].as_u64() < pair[1].as_u64()));
            assert!(covered.iter().all(|line| executable.contains(line)));
        }
        #[cfg(not(coverage))]
        assert!(report["coverage"]["error"]
            .as_str()
            .is_some_and(|message| message.contains("not compiled with")));
    }

    #[test]
    fn invokes_sequential_requests_with_public_input_and_setup_context() {
        let request = json!({
            "coverage_sources": ["src/validator.rs"],
            "requests": [
                {"key":{"case_id":"one","step_id":null},"input":{"value":1},"setup":"fixture-one","parallel":false},
                {"key":{"case_id":"two","step_id":"check"},"input":{"value":2},"setup":null,"parallel":false}
            ]
        });
        let visited = std::sync::Mutex::new(Vec::new());
        let report = run_dataset(&request, |input, context| {
            visited
                .lock()
                .unwrap()
                .push(context["case_id"].as_str().unwrap().to_owned());
            if context["case_id"] == "one" {
                assert_eq!(context["setup"], "fixture-one");
                assert_eq!(input, &json!({"value":1}));
            }
            Ok(input.clone())
        })
        .unwrap();

        assert_eq!(*visited.lock().unwrap(), ["one", "two"]);
        assert_eq!(report["results"][0]["key"]["case_id"], "one");
        assert_eq!(report["results"][0]["output"], json!({"value":1}));
        assert_eq!(report["results"][1]["key"]["step_id"], "check");
        assert_eq!(report["results"][1]["output"], json!({"value":2}));
    }

    #[test]
    fn invokes_marked_steps_concurrently_and_keeps_result_keys() {
        let request = json!({"requests":[
            {"key":{"case_id":"flow","step_id":"left"},"input":{"value":1},"parallel":true},
            {"key":{"case_id":"flow","step_id":"right"},"input":{"value":2},"parallel":true}
        ]});
        let barrier = std::sync::Barrier::new(2);
        let report = run_dataset(&request, |input, context| {
            assert_eq!(context["parallel"], true);
            barrier.wait();
            Ok(input.clone())
        })
        .unwrap();

        assert_eq!(report["results"].as_array().unwrap().len(), 2);
        assert_eq!(report["results"][0]["key"]["step_id"], "left");
        assert_eq!(report["results"][1]["key"]["step_id"], "right");
    }

    #[test]
    fn returns_validator_errors_as_keyed_results_and_continues() {
        let request = json!({"requests":[
            {"key":{"case_id":"bad","step_id":null},"input":{},"parallel":false},
            {"key":{"case_id":"good","step_id":null},"input":{},"parallel":false}
        ]});
        let report = run_dataset(&request, |_, context| {
            if context["case_id"] == "bad" {
                Err("invalid input".into())
            } else {
                Ok(Value::Null)
            }
        })
        .unwrap();

        assert_eq!(report["results"][0]["error"], "invalid input");
        assert_eq!(report["results"][1]["output"], Value::Null);
    }
}
