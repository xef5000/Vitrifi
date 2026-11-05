//! Test evidence is an explicit command, separate from compiling architecture.
use crate::{
    compiler::{relative_path, workspace_digest},
    model::*,
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn save(root: &Path, name: &str, value: &Value) -> Result<(), String> {
    let directory = root.join(".vitrifi");
    if directory.is_symlink() {
        return Err("Refusing to write through symlinked .vitrifi directory".into());
    }
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let temporary = directory.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|e| e.to_string())?;
    fs::rename(temporary, directory.join(name)).map_err(|e| e.to_string())
}
pub fn status(root: &Path) -> Result<Value, String> {
    let mut saved: Value = serde_json::from_slice(
        &fs::read(root.join(".vitrifi/evidence.json"))
            .map_err(|_| "No evidence recorded; run vitrifi verify")?,
    )
    .map_err(|e| e.to_string())?;
    if !saved.is_object() || !saved["workspace_revision"].is_string() {
        return Err("Invalid evidence record; run vitrifi verify to regenerate it".into());
    }
    let fresh = saved["workspace_revision"].as_str() == Some(workspace_digest(root)?.as_str());
    saved["fresh"] = json!(fresh);
    if !fresh {
        saved["recorded_status"] = saved["status"].clone();
        saved["status"] = json!("stale");
        if let Some(requirements) = saved["requirements"].as_array_mut() {
            for r in requirements {
                if r["checks"].as_array().is_some_and(|a| !a.is_empty()) {
                    r["status"] = json!("stale");
                }
            }
        }
    }
    Ok(saved)
}
fn run(root: &Path, module: &Module, declaration: &Declaration) -> Result<Value, String> {
    let DeclBody::Check {
        command,
        cwd,
        timeout,
        ..
    } = &declaration.body
    else {
        unreachable!()
    };
    let directory = root.join(relative_path(&module.path, cwd)?);
    let output_path = root.join(".vitrifi").join(format!(
        "run-{}-{}.log",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..])
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let start = Instant::now();
    let mut state = "failed";
    let mut exit = None;
    let mut failure = None;
    match cmd.spawn() {
        Err(e) => failure = Some(e.to_string()),
        Ok(mut child) => loop {
            match child.try_wait() {
                Ok(Some(result)) => {
                    exit = result.code();
                    if result.success() {
                        state = "passed";
                    }
                    break;
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    failure = Some(e.to_string());
                    break;
                }
                Ok(None) => {
                    if start.elapsed() >= Duration::from_secs(*timeout) {
                        #[cfg(unix)]
                        unsafe {
                            libc::kill(-(child.id() as i32), libc::SIGKILL);
                        }
                        #[cfg(not(unix))]
                        {
                            let _ = child.kill();
                        }
                        let _ = child.wait();
                        state = "timeout";
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        },
    }
    let mut bytes = vec![];
    File::open(&output_path)
        .map_err(|e| e.to_string())?
        .take(32001)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let truncated = bytes.len() > 32000;
    bytes.truncate(32000);
    let _ = fs::remove_file(output_path);
    Ok(
        json!({"id":format!("{}::{}",module.name,declaration.name),"status":state,"command":command,"cwd":directory,"exit_code":exit,
        "output":failure.unwrap_or_else(||String::from_utf8_lossy(&bytes).into()),"output_truncated":truncated,"seconds":start.elapsed().as_secs_f64()}),
    )
}
pub fn verify(root: &Path, program: &Program) -> Result<Value, String> {
    if !program.diagnostics.is_empty() {
        return Err("Compilation failed; no verification commands were executed".into());
    }
    for (path, hash) in &program.inputs {
        let raw = fs::read(root.join(path)).map_err(|e| e.to_string())?;
        if crate::compiler::hash_bytes(&raw) != *hash {
            return Err("Source changed after compilation; recompile before verification".into());
        }
    }
    let revision = workspace_digest(root)?;
    // Also establishes a safe output directory before launching commands.
    save(
        root,
        "verification-start.json",
        &json!({"revision":revision}),
    )?;
    let mut results = vec![];
    for module in &program.modules {
        for declaration in &module.declarations {
            if matches!(declaration.body, DeclBody::Check { .. }) {
                results.push(run(root, module, declaration)?);
            }
        }
    }
    let fresh = revision == workspace_digest(root)?;
    let status = if !fresh {
        "stale"
    } else if results.is_empty() {
        "not_run"
    } else if results.iter().all(|r| r["status"] == "passed") {
        "passed"
    } else {
        "failed"
    };
    let mut requirements = vec![];
    for module in &program.modules {
        for declaration in &module.declarations {
            if let DeclBody::Requirement { text } = &declaration.body {
                let id = format!("{}::{}", module.name, declaration.name);
                let checks: Vec<_> = program
                    .edges
                    .iter()
                    .filter(|e| e.to == id && e.relation == "covers")
                    .map(|e| e.from.clone())
                    .collect();
                let state = if checks.is_empty() {
                    "declared_unchecked"
                } else if !fresh {
                    "stale"
                } else if checks.iter().all(|id| {
                    results
                        .iter()
                        .any(|r| r["id"] == *id && r["status"] == "passed")
                }) {
                    "covered_by_passing_checks"
                } else {
                    "check_failed"
                };
                requirements.push(json!({"id":id,"text":text,"status":state,"checks":checks}));
            }
        }
    }
    let result = json!({"status":status,"fresh":fresh,"workspace_revision":revision,"compilation_revision":program.revision,"results":results,"requirements":requirements,
        "scope":"Evidence fingerprints nonignored repository files, including tests and feature source. Toolchains, ignored files and external systems are outside the fingerprint. Passing check exit codes support authored requirement links; they are not proofs or signed attestations."});
    save(root, "evidence.json", &result)?;
    Ok(result)
}
