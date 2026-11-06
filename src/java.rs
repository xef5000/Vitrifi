//! Java source analysis is an explicit compiler invocation, never an application run.
use crate::{compiler, derived, derived_model::*};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct Options {
    pub analyzer: PathBuf,
    pub classpath_file: PathBuf,
    pub source_root: PathBuf,
    pub java: PathBuf,
}

#[derive(Serialize)]
pub struct Analysis {
    pub program: derived::DerivedProgram,
    pub revision: String,
    pub inputs: BTreeMap<String, String>,
    pub scope: Vec<String>,
}

fn absolute(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn hash_classpath(path: &Path, inputs: &mut BTreeMap<String, String>) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("Classpath entry {}: {e}", path.display()))?;
    if path.is_file() {
        inputs.insert(
            format!("dependency:{}", path.display()),
            compiler::hash_bytes(&fs::read(&path).map_err(|e| e.to_string())?),
        );
    } else if path.is_dir() {
        inputs.insert(
            format!("directory:{}", path.display()),
            directory_digest(&path)?,
        );
    } else {
        return Err(format!("Invalid classpath entry {}", path.display()));
    }
    Ok(())
}

fn directory_digest(path: &Path) -> Result<String, String> {
    let mut contents = BTreeMap::new();
    for item in walkdir::WalkDir::new(&path).follow_links(false) {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_type().is_symlink() {
            return Err(format!(
                "Symlinked classpath content is unsupported: {}",
                item.path().display()
            ));
        }
        if item.file_type().is_file() {
            contents.insert(
                compiler::slash(item.path().strip_prefix(path).map_err(|e| e.to_string())?),
                compiler::hash_bytes(&fs::read(item.path()).map_err(|e| e.to_string())?),
            );
        }
    }
    Ok(compiler::content_digest(&contents))
}

fn source_paths(root: &Path, options: &Options) -> Result<Vec<String>, String> {
    let directory = absolute(root, &options.source_root)
        .canonicalize()
        .map_err(|_| {
            format!(
                "Java source directory {} does not exist; set --source-root",
                options.source_root.display()
            )
        })?;
    if !directory.starts_with(root) {
        return Err("Java source directory must be inside --root".into());
    }
    let mut paths = Vec::new();
    for path in compiler::repository_files(root)? {
        if path.ends_with(".java") && root.join(&path).starts_with(&directory) {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err("No Java sources found in --source-root".into());
    }
    Ok(paths)
}

pub fn analyze(root: &Path, options: &Options) -> Result<Analysis, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let analyzer = absolute(&root, &options.analyzer).canonicalize()
        .map_err(|_| "Java analyzer JAR is missing; build integrations/java and set --java-analyzer or VITRIFI_JAVA_ANALYZER")?;
    let classpath_file = absolute(&root, &options.classpath_file);
    let raw = fs::read_to_string(&classpath_file).map_err(|e| {
        format!(
            "Cannot read classpath {}: {e}; run the Maven build first",
            classpath_file.display()
        )
    })?;
    let mut classpath = Vec::new();
    let mut inputs = BTreeMap::new();
    inputs.insert(
        format!(
            "dependency:{}",
            classpath_file
                .canonicalize()
                .map_err(|e| e.to_string())?
                .display()
        ),
        compiler::hash_bytes(raw.as_bytes()),
    );
    inputs.insert(
        format!("dependency:{}", analyzer.display()),
        compiler::hash_bytes(&fs::read(&analyzer).map_err(|e| e.to_string())?),
    );
    for item in std::env::split_paths(raw.trim()) {
        if item.as_os_str().is_empty() {
            continue;
        }
        let path = absolute(&root, &item)
            .canonicalize()
            .map_err(|e| format!("Classpath {}: {e}", item.display()))?;
        hash_classpath(&path, &mut inputs)?;
        classpath.push(path);
    }
    classpath.push(analyzer.clone());
    let classpath = std::env::join_paths(classpath).map_err(|e| e.to_string())?;
    let sources = source_paths(&root, options)?;
    for path in &sources {
        inputs.insert(
            path.clone(),
            compiler::hash_bytes(compiler::read_inside(&root, path)?.as_bytes()),
        );
    }
    let temporary = tempfile::tempdir().map_err(|e| e.to_string())?;
    let manifest = temporary.path().join("sources.txt");
    fs::write(&manifest, sources.join("\n")).map_err(|e| e.to_string())?;
    let output = Command::new(&options.java)
        .args(["-jar"])
        .arg(&analyzer)
        .arg("--root")
        .arg(&root)
        .arg("--classpath")
        .arg(classpath)
        .arg("--sources-file")
        .arg(&manifest)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            format!(
                "Cannot run Java compiler: {e}; use a full JDK 21+ and JAVA_HOME or VITRIFI_JAVA"
            )
        })?;
    let source_analysis: SourceAnalysis = serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "Java analyzer did not return a valid model: {e}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    if !output.status.success() && source_analysis.diagnostics.is_empty() {
        return Err(format!(
            "Java analyzer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    if !inputs_current(&root, &inputs) || source_paths(&root, options)? != sources {
        return Err("Source or classpath changed during analysis; retry".into());
    }
    // Include source content, analyzer, and the ordered classpath selection.
    let mut identity = BTreeMap::new();
    for (key, value) in &inputs {
        let key = if key.starts_with("dependency:") || key.starts_with("directory:") {
            format!("dependency:{value}")
        } else {
            key.clone()
        };
        identity.insert(key, value.clone());
    }
    let revision = compiler::content_digest(&identity);
    let program = derived::build(source_analysis);
    Ok(Analysis { program, revision, inputs, scope: vec![
        "Generated from javac-attributed Java sources with annotation processing disabled; application code is not executed.".into(),
        "Spring resolution covers supported source-visible constructor injection in this source set; unresolved dispatch remains explicit.".into(),
        "SQL effects cover recognized JdbcTemplate operations and ResourceTable mappings; unmodeled code prevents a complete static claim.".into(),
        "Runtime policies require instrumented test executions; generation does not establish behavior on unexecuted paths.".into(),
    ] })
}

pub fn output_paths(analysis: &Analysis) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for (name, feature) in &analysis.program.features {
        if name.split('.').any(|p| {
            p.is_empty()
                || !p.chars().enumerate().all(|(i, c)| {
                    c == '_' || c.is_ascii_alphabetic() || i > 0 && c.is_ascii_digit()
                })
        }) {
            return Err(format!("Invalid feature name {name:?}"));
        }
        let first = feature
            .roots
            .first()
            .and_then(|id| analysis.program.operations.get(id))
            .ok_or("Feature has no source entry")?;
        let parent = Path::new(&first.source.path)
            .parent()
            .unwrap_or(Path::new(""));
        result.insert(
            name.clone(),
            compiler::slash(&parent.join(format!("{name}.vitrifi"))),
        );
    }
    Ok(result)
}

pub fn artifacts(analysis: &Analysis) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    for (feature, path) in output_paths(analysis)? {
        let text = derived::render(&analysis.program, &feature, &path)?;
        result.insert(path, text);
    }
    Ok(result)
}

pub fn static_status(analysis: &Analysis) -> &'static str {
    if !analysis.program.diagnostics.is_empty() {
        "failed"
    } else if analysis.program.features.is_empty() {
        "no_features"
    } else if analysis
        .program
        .features
        .values()
        .flat_map(|f| &f.operations)
        .any(|id| {
            !analysis.program.operations[id]
                .reachable_unknowns
                .is_empty()
        })
    {
        "incomplete"
    } else {
        "passed"
    }
}

pub fn generate(root: &Path, analysis: &Analysis, check_only: bool) -> Result<Value, String> {
    let outputs = artifacts(analysis)?;
    let mut stale = Vec::new();
    let mut written = Vec::new();
    for (path, expected) in &outputs {
        let target = root.join(path);
        let existing = match fs::read_to_string(&target) {
            Ok(value) => Some(value),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Cannot read existing generated output {path}: {e}")),
        };
        if existing.as_deref() == Some(expected) {
            continue;
        }
        if check_only {
            stale.push(path.clone());
            continue;
        }
        if target.is_symlink()
            || existing
                .as_ref()
                .is_some_and(|s| !s.starts_with("// @generated by vitrifi source analysis"))
        {
            return Err(format!(
                "Refusing to replace authored or symlinked file {path}"
            ));
        }
        let parent = target.parent().ok_or("Invalid output path")?;
        if parent.canonicalize().map_err(|e| e.to_string())? != parent {
            return Err("Refusing a symlinked output directory".into());
        }
        fs::write(&target, expected).map_err(|e| e.to_string())?;
        written.push(path.clone());
    }
    // Detect artifacts left behind after deleting or renaming a source feature.
    let mut obsolete = Vec::new();
    for path in compiler::repository_files(root)? {
        if path.ends_with(".vitrifi") && !outputs.contains_key(&path) {
            if compiler::read_inside(root, &path)?
                .starts_with("// @generated by vitrifi source analysis")
            {
                obsolete.push(path);
            }
        }
    }
    // Never delete a file automatically. Report obsolete snapshots for explicit cleanup.
    let status = if !stale.is_empty() || !obsolete.is_empty() {
        "stale"
    } else {
        static_status(analysis)
    };
    let unresolved: BTreeMap<_, _> = analysis
        .program
        .features
        .values()
        .flat_map(|f| &f.roots)
        .filter_map(|id| {
            let op = &analysis.program.operations[id];
            if op.reachable_unknowns.is_empty() {
                None
            } else {
                Some((id, &op.reachable_unknowns))
            }
        })
        .collect();
    Ok(
        json!({"status":status,"revision":analysis.revision,"files":outputs.keys().collect::<Vec<_>>(),"written":written,"stale":stale,"obsolete":obsolete,"diagnostics":analysis.program.diagnostics,"policy_results":analysis.program.policy_results,"unresolved":unresolved,"scope":analysis.scope}),
    )
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().into())
    }
}

pub fn review(root: &Path, options: &Options, base: &str) -> Result<Value, String> {
    let current = analyze(root, options)?;
    let top = String::from_utf8(git(root, &["rev-parse", "--show-toplevel"])?)
        .map_err(|e| e.to_string())?;
    let top = Path::new(top.trim())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let prefix = compiler::slash(root.strip_prefix(&top).map_err(|e| e.to_string())?);
    let revision = String::from_utf8(git(
        root,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )?)
    .map_err(|e| e.to_string())?
    .trim()
    .to_string();
    let snapshot = tempfile::tempdir().map_err(|e| e.to_string())?;
    let source_relative = absolute(root, &options.source_root)
        .canonicalize()
        .map_err(|e| e.to_string())?
        .strip_prefix(root)
        .map_err(|_| "Source root outside repository")?
        .to_path_buf();
    let source_prefix = compiler::slash(&source_relative);
    for raw in git(&top, &["ls-tree", "-r", "--name-only", "-z", &revision])?
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
    {
        let full = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
        let relative = if prefix.is_empty() {
            Some(full)
        } else {
            full.strip_prefix(&format!("{prefix}/"))
        };
        if let Some(path) = relative {
            if !path.ends_with(".java") || !Path::new(path).starts_with(&source_prefix) {
                continue;
            }
            let destination = snapshot.path().join(path);
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(
                destination,
                git(&top, &["show", &format!("{revision}:{full}")])?,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    let mut historical_options = options.clone();
    historical_options.analyzer = absolute(root, &options.analyzer);
    historical_options.classpath_file = absolute(root, &options.classpath_file);
    historical_options.source_root = source_relative.clone();
    // Rebase relative classpath entries against the live project, not the snapshot.
    let original_cp =
        fs::read_to_string(&historical_options.classpath_file).map_err(|e| e.to_string())?;
    let resolved_cp = std::env::join_paths(
        std::env::split_paths(original_cp.trim())
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| absolute(root, &p)),
    )
    .map_err(|e| e.to_string())?;
    let historical_cp = snapshot.path().join("classpath.txt");
    fs::write(&historical_cp, resolved_cp.as_encoded_bytes()).map_err(|e| e.to_string())?;
    historical_options.classpath_file = historical_cp;
    let historical_source = snapshot.path().join(&source_relative);
    let before = if historical_source.exists() {
        Some(analyze(snapshot.path(), &historical_options)?)
    } else {
        None
    };
    let before_facts = before
        .as_ref()
        .map(|a| derived::facts(&a.program))
        .unwrap_or_default();
    let after_facts = derived::facts(&current.program);
    let mut changes = Vec::new();
    for key in before_facts
        .keys()
        .chain(after_facts.keys())
        .collect::<BTreeSet<_>>()
    {
        if before_facts.get(key) != after_facts.get(key) {
            changes.push(json!({"fact":key,"change":if !before_facts.contains_key(key){"added"}else if !after_facts.contains_key(key){"removed"}else{"modified"},"before":before_facts.get(key),"after":after_facts.get(key)}));
        }
    }
    let base_status = before.as_ref().map(static_status).unwrap_or("absent");
    Ok(
        json!({"base":revision,"status":if matches!(static_status(&current),"passed") && matches!(base_status,"passed"|"absent") {"passed"}else{"incomplete"},"changes":changes,"before_status":base_status,"after_status":static_status(&current),"before_diagnostics":before.as_ref().map(|a| &a.program.diagnostics),"after_diagnostics":current.program.diagnostics,"scope":"Both source revisions are analyzed without executing repository code, using the current analyzer and dependency classpath. This compares implementation-derived facts, not committed generated snapshots."}),
    )
}

pub fn verify(
    root: &Path,
    analysis: &Analysis,
    runner: &[String],
    timeout: u64,
) -> Result<Value, String> {
    if runner.is_empty() {
        return Err("Java verification requires an explicit test command after --".into());
    }
    if timeout == 0 || timeout > 3600 {
        return Err("--timeout must be 1..3600 seconds".into());
    }
    if static_status(analysis) != "passed" {
        return Ok(json!({"status":static_status(analysis),"executed":false,"analysis":analysis}));
    }
    let workspace_revision = compiler::workspace_digest(root)?;
    let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
    let trace = directory.path().join("trace.json");
    let session_id = format!(
        "vitrifi-{}-{}",
        std::process::id(),
        directory.path().file_name().unwrap().to_string_lossy()
    );
    let log = directory.path().join("runner.log");
    let stdout = fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&runner[0]);
    cmd.args(&runner[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| e.to_string())?)
        .stderr(stdout)
        .env("VITRIFI_TRACE_FILE", &trace)
        .env("VITRIFI_SESSION_ID", &session_id);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Cannot start test command: {e}"))?;
    let start = Instant::now();
    let mut timed_out = false;
    let exit_code = loop {
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            break exit.code();
        }
        if start.elapsed() > Duration::from_secs(timeout) {
            timed_out = true;
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let trace_value: Option<Value> = fs::read(&trace)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let fresh = workspace_revision == compiler::workspace_digest(root)?
        && inputs_current(root, &analysis.inputs);
    let evidence_gaps = runtime_gaps(analysis, trace_value.as_ref(), &session_id);
    let evidence_valid = evidence_gaps.is_empty();
    let runtime_failed = trace_value.as_ref().is_some_and(|v| {
        v["status"] == "failed" || v["violations"].as_array().is_some_and(|a| !a.is_empty())
    });
    let status = if !fresh {
        "stale"
    } else if timed_out {
        "timeout"
    } else if exit_code != Some(0) || runtime_failed {
        "failed"
    } else if evidence_valid {
        "passed"
    } else {
        "incomplete"
    };
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(&log)
        .map_err(|e| e.to_string())?
        .take(32000)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let report = json!({"status":status,"fresh":fresh,"revision":analysis.revision,"workspace_revision":workspace_revision,"inputs":analysis.inputs,"command":runner,"exit_code":exit_code,"output":String::from_utf8_lossy(&bytes),"runtime":trace_value,"evidence_gaps":evidence_gaps,"scope":"Evidence for captured synchronous Spring/JDBC test executions. It does not prove unexecuted paths, bypassed data sources, asynchronous work, or external effects."});
    crate::evidence::save(root, "java-evidence.json", &report)?;
    Ok(report)
}

fn runtime_gaps(analysis: &Analysis, report: Option<&Value>, session: &str) -> Vec<String> {
    let mut gaps = Vec::new();
    let Some(report) = report else {
        return vec!["Test command produced no valid runtime evidence".into()];
    };
    if report["schema_version"] != 1
        || report["session_id"] != session
        || report["complete"] != true
        || report["status"] != "passed"
    {
        gaps.push("Runtime evidence is incomplete, failed, or belongs to a different verification session".into());
    }
    if !report["scenarios"]
        .as_array()
        .is_some_and(|s| !s.is_empty() && s.iter().all(|s| s["outcome"] == "passed"))
    {
        gaps.push("No complete passing scenarios".into());
    }
    if !report["violations"].as_array().is_some_and(Vec::is_empty) {
        gaps.push("Runtime policy violations or missing violation record".into());
    }
    if !report["gaps"].as_array().is_some_and(Vec::is_empty) {
        gaps.push("Runtime instrumentation gaps or missing gap record".into());
    }
    let invocations = report["invocations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if invocations.is_empty() {
        gaps.push("No feature invocations were recorded".into());
    }
    let coverage = report["policy_coverage"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut checked = BTreeSet::new();
    for feature in analysis.program.features.values() {
        for id in &feature.operations {
            let operation = &analysis.program.operations[id];
            let owner = operation
                .source
                .feature
                .as_ref()
                .map(|f| f.name.as_str())
                .unwrap_or(feature.name.as_str());
            for policy in &operation.source.policies {
                let (kind, effect, needs_error) = match policy {
                    SourcePolicy::Forbid { effect } => ("forbid", effect, false),
                    SourcePolicy::NoEffectOnError { effect } => {
                        ("no_effect_on_error", effect, true)
                    }
                };
                if !checked.insert((id, kind, effect.clone())) {
                    continue;
                }
                let matches =
                    |v: &Value| v["feature"] == owner && v["method"] == operation.source.symbol;
                let covered = coverage.iter().any(|v| {
                    matches(v)
                        && v["kind"] == kind
                        && v["action"] == effect.action
                        && v["resource"] == effect.resource
                        && v["status"] == "passed"
                });
                let exercised = invocations.iter().any(|v| {
                    matches(v)
                        && (!needs_error || v["outcome"] == "threw")
                        && v["policies"].as_array().is_some_and(|ps| {
                            ps.iter().any(|p| {
                                p["kind"] == kind
                                    && p["action"] == effect.action
                                    && p["resource"] == effect.resource
                                    && p["status"] == "passed"
                            })
                        })
                });
                if !covered || !exercised {
                    gaps.push(format!(
                        "Policy {kind} {} {} on {} has no matching {}execution evidence",
                        effect.action,
                        effect.resource,
                        operation.source.symbol,
                        if needs_error { "error-path " } else { "" }
                    ));
                }
            }
        }
    }
    gaps
}

fn inputs_current(root: &Path, inputs: &BTreeMap<String, String>) -> bool {
    inputs.iter().all(|(key, hash)| {
        if let Some(directory) = key.strip_prefix("directory:") {
            return directory_digest(Path::new(directory)).is_ok_and(|s| s == *hash);
        }
        let path = key
            .strip_prefix("dependency:")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join(key));
        fs::read(path).is_ok_and(|b| compiler::hash_bytes(&b) == *hash)
    })
}

pub fn evidence_status(root: &Path) -> Result<Value, String> {
    let mut report: Value = serde_json::from_slice(
        &fs::read(root.join(".vitrifi/java-evidence.json"))
            .map_err(|_| "No Java evidence; run verify --java")?,
    )
    .map_err(|e| e.to_string())?;
    let inputs: BTreeMap<String, String> = serde_json::from_value(report["inputs"].clone())
        .map_err(|_| "Invalid Java evidence inputs")?;
    let fresh = report["workspace_revision"].as_str()
        == Some(compiler::workspace_digest(root)?.as_str())
        && inputs_current(root, &inputs);
    report["fresh"] = json!(fresh);
    if !fresh {
        report["recorded_status"] = report["status"].clone();
        report["status"] = json!("stale");
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis() -> Analysis {
        let operation = SourceOperation {
            id: "Checkout.java::example.Checkout.apply()".into(),
            path: "Checkout.java".into(),
            symbol: "example.Checkout.apply()".into(),
            language: "java".into(),
            line: 1,
            signature: "void apply()".into(),
            parameters: vec![],
            returns: "void".into(),
            feature: Some(FeatureMarker {
                name: "Checkout".into(),
                purpose: "Apply a coupon".into(),
            }),
            calls: vec![],
            direct_effects: vec![],
            policies: vec![SourcePolicy::NoEffectOnError {
                effect: ResourceEffect {
                    action: "write".into(),
                    resource: "Carts".into(),
                },
            }],
        };
        Analysis {
            program: derived::build(SourceAnalysis {
                operations: vec![operation],
                diagnostics: vec![],
            }),
            revision: "revision".into(),
            inputs: BTreeMap::new(),
            scope: vec![],
        }
    }

    fn report() -> Value {
        let policy = json!({"feature":"Checkout","method":"example.Checkout.apply()","kind":"no_effect_on_error","action":"write","resource":"Carts","status":"passed"});
        json!({"schema_version":1,"session_id":"session","complete":true,"status":"passed",
            "scenarios":[{"name":"invalid coupon","outcome":"passed"}],
            "invocations":[{"feature":"Checkout","method":"example.Checkout.apply()","scenario":"invalid coupon","outcome":"threw","events":[],"policies":[policy.clone()]}],
            "violations":[],"gaps":[],"policy_coverage":[policy]})
    }

    #[test]
    fn runtime_evidence_must_exercise_the_expected_error_path() {
        let analysis = analysis();
        let mut value = report();
        assert!(runtime_gaps(&analysis, Some(&value), "session").is_empty());
        value["invocations"][0]["outcome"] = json!("returned");
        assert!(runtime_gaps(&analysis, Some(&value), "session")
            .iter()
            .any(|g| g.contains("error-path")));
    }

    #[test]
    fn a_passing_unrelated_session_cannot_cover_source_policies() {
        let analysis = analysis();
        let mut value = report();
        value["invocations"][0]["method"] = json!("example.Other.run()");
        value["policy_coverage"] = json!([]);
        assert!(!runtime_gaps(&analysis, Some(&value), "session").is_empty());
    }

    #[test]
    fn incomplete_missing_or_replayed_reports_do_not_pass() {
        let analysis = analysis();
        assert!(!runtime_gaps(&analysis, None, "session").is_empty());
        assert!(!runtime_gaps(&analysis, Some(&report()), "another-session").is_empty());
        let mut value = report();
        value["complete"] = json!(false);
        assert!(!runtime_gaps(&analysis, Some(&value), "session").is_empty());
    }
}
