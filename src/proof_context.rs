//! Reconstruct certificate assumptions from Git and current compiler inputs.
//! Historical files are data only; historical builds are never executed.
use crate::{compiler, derived, java, proof_kernel, target, targets, workflow, workflows};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub kind: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub source_root: String,
    pub analyzer_sha256: String,
    /// Order and multiplicity are significant, machine-specific paths are not.
    pub classpath: Vec<Dependency>,
    pub java_runtime: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub repository_root: String,
    pub files: BTreeMap<String, String>,
    pub targets: Vec<String>,
    pub environment: Environment,
}

pub struct Pair {
    pub commit: String,
    pub path: String,
    pub before_source: String,
    pub after_source: String,
    pub before: workflow::Workflow,
    pub after: workflow::Workflow,
    git: Git,
}

fn git_command(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "PROOF_GIT: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
pub fn resolve(root: &Path, reference: &str) -> Result<String, String> {
    let output = git_command(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    Ok(String::from_utf8(output)
        .map_err(|e| e.to_string())?
        .trim()
        .into())
}
struct Git {
    top: PathBuf,
    prefix: String,
    commit: String,
    files: BTreeMap<String, String>,
}
impl Git {
    fn new(root: &Path, reference: &str) -> Result<Self, String> {
        let top = String::from_utf8(git_command(root, &["rev-parse", "--show-toplevel"])?)
            .map_err(|e| e.to_string())?;
        let top = Path::new(top.trim())
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let prefix = compiler::slash(
            root.strip_prefix(&top)
                .map_err(|_| "PROOF_ROOT: root must be inside the Git repository")?,
        );
        let commit = resolve(root, reference)?;
        let mut files = BTreeMap::new();
        for raw in git_command(&top, &["ls-tree", "-r", "-z", "--full-tree", &commit])?
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
        {
            let entry = std::str::from_utf8(raw).map_err(|_| "PROOF_GIT: non-UTF-8 path")?;
            let (metadata, path) = entry
                .split_once('\t')
                .ok_or("PROOF_GIT: invalid tree entry")?;
            files.insert(
                path.into(),
                metadata.split_whitespace().next().unwrap_or("").into(),
            );
        }
        Ok(Self {
            top,
            prefix,
            commit,
            files,
        })
    }
    fn full(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.into()
        } else {
            format!("{}/{path}", self.prefix)
        }
    }
    fn bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        if !self
            .files
            .get(path)
            .is_some_and(|mode| matches!(mode.as_str(), "100644" | "100755"))
        {
            return Err(format!(
                "PROOF_BASELINE: missing or nonregular Git file {path}"
            ));
        }
        let bytes = git_command(&self.top, &["show", &format!("{}:{path}", self.commit)])?;
        if bytes.len() > 64_000_000 {
            return Err(format!("PROOF_LIMIT: baseline file {path} exceeds 64 MB"));
        }
        Ok(bytes)
    }
    fn read(&self, path: &str) -> Result<String, String> {
        String::from_utf8(self.bytes(path)?)
            .map_err(|_| format!("PROOF_BASELINE: expected UTF-8 file {path}"))
    }
}

pub fn relative(root: &Path, path: &Path) -> Result<String, String> {
    let path = if path.is_absolute() {
        path.strip_prefix(root)
            .map_err(|_| "PROOF_PATH: path must be inside --root")?
    } else {
        path
    };
    if path.as_os_str().is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("PROOF_PATH: use a normal relative path without traversal".into());
    }
    Ok(compiler::slash(path))
}
pub fn load_pair(root: &Path, path: &Path, reference: &str) -> Result<Pair, String> {
    let path = relative(root, path)?;
    if !path.ends_with(".workflow.vitrifi") {
        return Err("PROOF_PATH: select a .workflow.vitrifi file".into());
    }
    let git = Git::new(root, reference)?;
    let before_source = git.read(&git.full(&path))?;
    let after_source = compiler::read_inside(root, &path)?;
    let parse = |source: &str| {
        workflow::parse(&path, source).map_err(|d| serde_json::to_string(&d).unwrap())
    };
    let before = parse(&before_source)?;
    let after = parse(&after_source)?;
    proof_kernel::validate(&before)?;
    proof_kernel::validate(&after)?;
    if proof_kernel::contract(&before) != proof_kernel::contract(&after) {
        return Err(
            "PROOF_CONTRACT: only operation bodies and source formatting may change".into(),
        );
    }
    Ok(Pair {
        commit: git.commit.clone(),
        path,
        before_source,
        after_source,
        before,
        after,
        git,
    })
}

fn generated_path(
    root: &Path,
    options: &java::Options,
    workflow: &workflow::Workflow,
) -> Result<String, String> {
    let source = relative(root, &options.source_root)?;
    Ok(format!("{source}/{}.java", workflow.emit.replace('.', "/")))
}
fn excluded(path: &str, source: &[u8], pair: &Pair, generated: &str) -> bool {
    path == pair.path
        || path == generated
        || path.ends_with(".proof.json")
        || (path.ends_with(".vitrifi")
            && !path.ends_with(".target.vitrifi")
            && !path.ends_with(".workflow.vitrifi")
            && source.starts_with(derived::GENERATED_MARKER.as_bytes()))
}
fn ancestor_build_file(path: &str, prefix: &str) -> bool {
    // Maven parent configuration is outside a module's --root. Compare it too.
    let mut parent = Path::new(prefix).parent();
    while let Some(directory) = parent {
        if Path::new(path) == directory.join("pom.xml")
            || Path::new(path).starts_with(directory.join(".mvn"))
        {
            return true;
        }
        parent = directory.parent();
    }
    false
}
fn read_context_file(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let relative = relative(root, Path::new(path))?;
    let full = root.join(relative);
    if full.canonicalize().map_err(|e| e.to_string())? != full {
        return Err("PROOF_PATH: symlinked contextual input".into());
    }
    if fs::metadata(&full).map_err(|e| e.to_string())?.len() > 64_000_000 {
        return Err("PROOF_LIMIT: contextual file exceeds 64 MB".into());
    }
    fs::read(full).map_err(|e| e.to_string())
}

fn context_files(
    root: &Path,
    pair: &Pair,
    generated: &str,
    source_root: &str,
) -> Result<BTreeMap<String, String>, String> {
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    let prefix = if pair.git.prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", pair.git.prefix)
    };
    for path in pair.git.files.keys() {
        let relative = path.strip_prefix(&prefix);
        if relative.is_some_and(|path| {
            compiler::ignored(Path::new(path)) && !protected_source(path, source_root)
        }) {
            continue;
        }
        if relative.is_some_and(|path| path.ends_with(".proof.json")) {
            continue;
        }
        if relative.is_some() || ancestor_build_file(path, &pair.git.prefix) {
            let source = pair.git.bytes(path)?;
            if !relative.is_some_and(|path| excluded(path, &source, pair, generated)) {
                before.insert(path.clone(), compiler::hash_bytes(&source));
            }
        }
    }
    for path in working_paths(root, source_root)? {
        let source = read_context_file(root, &path)?;
        if !excluded(&path, &source, pair, generated) {
            after.insert(pair.git.full(&path), compiler::hash_bytes(&source));
        }
    }
    if !pair.git.prefix.is_empty() {
        for path in compiler::repository_files(&pair.git.top)? {
            if ancestor_build_file(&path, &pair.git.prefix) {
                let source = read_context_file(&pair.git.top, &path)?;
                after.insert(path, compiler::hash_bytes(&source));
            }
        }
    }
    if before != after {
        let changed: Vec<_> = before
            .keys()
            .chain(after.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|path| before.get(*path) != after.get(*path))
            .cloned()
            .collect();
        return Err(format!("PROOF_CONTEXT_CHANGED: unchanged native sources, other authored files and build configuration required; changed: {}",changed.join(", ")));
    }
    Ok(after)
}

fn protected_source(path: &str, source_root: &str) -> bool {
    Path::new(path).starts_with("src") || Path::new(path).starts_with(source_root)
}

/// Git ignore rules cannot hide changed native source/resources from a proof.
/// Ordinary generated build directories remain excluded outside source trees.
fn working_paths(root: &Path, source_root: &str) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            let path = compiler::slash(entry.path().strip_prefix(root).unwrap());
            !compiler::ignored(Path::new(&path)) || protected_source(&path, source_root)
        })
    {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = compiler::slash(entry.path().strip_prefix(root).unwrap());
        if path.ends_with(".proof.json") {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(format!("PROOF_PATH: symlinked contextual input {path}"));
        }
        if entry.file_type().is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn dependency(path: &Path) -> Result<Dependency, String> {
    if path.is_file() {
        Ok(Dependency {
            kind: "file".into(),
            sha256: compiler::hash_bytes(&fs::read(path).map_err(|e| e.to_string())?),
        })
    } else if path.is_dir() {
        let mut inputs = BTreeMap::new();
        for item in walkdir::WalkDir::new(path).follow_links(false) {
            let item = item.map_err(|e| e.to_string())?;
            if item.file_type().is_symlink() {
                return Err("PROOF_DEPENDENCY: symlinked directory content unsupported".into());
            }
            if item.file_type().is_file() {
                inputs.insert(
                    compiler::slash(item.path().strip_prefix(path).unwrap()),
                    compiler::hash_bytes(&fs::read(item.path()).map_err(|e| e.to_string())?),
                );
            }
        }
        Ok(Dependency {
            kind: "directory".into(),
            sha256: compiler::content_digest(&inputs),
        })
    } else {
        Err(format!("PROOF_DEPENDENCY: missing {}", path.display()))
    }
}
fn environment(root: &Path, options: &java::Options) -> Result<Environment, String> {
    let absolute = |path: &Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
    };
    let raw = fs::read_to_string(absolute(&options.classpath_file)).map_err(|e| e.to_string())?;
    let mut classpath = Vec::new();
    for path in std::env::split_paths(raw.trim()).filter(|p| !p.as_os_str().is_empty()) {
        classpath.push(dependency(
            &absolute(&path).canonicalize().map_err(|e| e.to_string())?,
        )?);
    }
    let analyzer_sha256 =
        compiler::hash_bytes(&fs::read(absolute(&options.analyzer)).map_err(|e| e.to_string())?);
    let output = Command::new(&options.java)
        .args(["-XshowSettings:properties", "-version"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("PROOF_JAVA: cannot identify selected Java runtime".into());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    let mut java_runtime = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.trim().split_once(" = ") {
            if [
                "java.runtime.version",
                "java.vendor",
                "java.vm.name",
                "java.specification.version",
                "os.arch",
            ]
            .contains(&key)
            {
                java_runtime.insert(key.into(), value.into());
            }
        }
    }
    if !java_runtime.contains_key("java.runtime.version") {
        return Err("PROOF_JAVA: missing runtime version".into());
    }
    Ok(Environment {
        source_root: relative(root, &options.source_root)?,
        analyzer_sha256,
        classpath,
        java_runtime,
    })
}

/// Compile both endpoints against the same native source/dependency context.
/// Recompute this during verification; certificate-supplied paths are not used
/// for external reads or commands.
pub fn capture(
    root: &Path,
    pair: &Pair,
    options: &java::Options,
    requested_targets: &[PathBuf],
) -> Result<Context, String> {
    let target_discovery = targets::discover(root)?;
    let source_root = relative(root, &options.source_root)?;
    let generated = generated_path(root, options, &pair.after)?;
    let before_java =
        workflow::render(&pair.before).map_err(|d| serde_json::to_string(&d).unwrap())?;
    if pair.git.read(&pair.git.full(&generated))? != before_java {
        return Err(
            "PROOF_BASELINE_STALE: committed service must match the baseline workflow".into(),
        );
    }
    let files = context_files(root, pair, &generated, &source_root)?;
    let actual_java: Vec<_> = working_paths(root, &source_root)?
        .into_iter()
        .filter(|path| path.ends_with(".java") && Path::new(path).starts_with(&source_root))
        .collect();
    if actual_java != java::source_paths(root, options)? {
        return Err("PROOF_SOURCE_COVERAGE: every Java file in the source tree must be included by the adapter; ignored sources cannot be certified".into());
    }
    let environment_before = environment(root, options)?;
    let mut paths = workflows::discover(root)?;
    paths.push(pair.path.clone().into());
    let set = workflows::load(root, &paths)?;
    let current = java::analyze_with_workflows(root, options, set)?;
    let historical = java::analyze_compiled(
        root,
        options,
        &BTreeMap::from([(generated.clone(), before_java)]),
    )?;
    let errors = workflow::check_leaves(&pair.before, &historical.program);
    if !errors.is_empty() {
        return Err(format!(
            "PROOF_BASELINE_ANALYSIS: {}",
            serde_json::to_string(&errors).unwrap()
        ));
    }
    for analysis in [&current, &historical] {
        if java::static_status(analysis) != "passed" {
            return Err(format!(
                "PROOF_ANALYSIS: endpoint compilation or reachable call analysis incomplete: {}",
                serde_json::to_string(&analysis.program.diagnostics).unwrap()
            ));
        }
    }
    let targets = if requested_targets.is_empty() {
        target_discovery.clone()
    } else {
        requested_targets.to_vec()
    };
    let selected = if targets.is_empty() {
        None
    } else {
        Some(targets::load(root, &targets)?)
    };
    if let Some(set) = &selected {
        if !set.diagnostics.is_empty() {
            return Err(serde_json::to_string(&set.diagnostics).unwrap());
        }
        for analysis in [&current, &historical] {
            let report = target::conform(&set.features, &analysis.program);
            if report.status != "passed" {
                return Err(format!(
                    "PROOF_TARGET: {}",
                    serde_json::to_string(&report).unwrap()
                ));
            }
        }
        // Explicit ignored targets must also have an unchanged Git baseline.
        for (path, hash) in &set.inputs {
            if compiler::hash_bytes(pair.git.read(&pair.git.full(path))?.as_bytes()) != *hash {
                return Err(format!("PROOF_TARGET_CHANGED: {path}"));
            }
        }
    }
    let target_files = selected
        .as_ref()
        .map(|s| s.files.clone())
        .unwrap_or_default();
    if context_files(root, pair, &generated, &source_root)? != files
        || environment(root, options)? != environment_before
        || compiler::read_inside(root, &pair.path)? != pair.after_source
        || !java::inputs_current(root, &current.inputs)
        || !java::inputs_current(root, &historical.inputs)
        || selected.as_ref().is_some_and(|s| !s.current(root))
        || (requested_targets.is_empty() && targets::discover(root)? != target_discovery)
    {
        return Err("PROOF_STALE: inputs changed during proof validation".into());
    }
    Ok(Context {
        repository_root: pair.git.prefix.clone(),
        files,
        targets: target_files,
        environment: environment_before,
    })
}
