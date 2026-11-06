//! Safe, deterministic compilation of feature-local executable workflows.
use crate::{compiler, java, model::Diagnostic, target, targets, workflow};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub struct WorkflowSet {
    pub features: Vec<workflow::Workflow>,
    pub inputs: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    discovered: bool,
}

pub fn discover(root: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(compiler::repository_files(root)?
        .into_iter()
        .filter(|p| p.ends_with(".workflow.vitrifi"))
        .map(PathBuf::from)
        .collect())
}
pub fn selection_digest(root: &Path) -> Result<String, String> {
    Ok(compiler::hash_bytes(
        &serde_json::to_vec(&discover(root)?).unwrap(),
    ))
}
pub fn load(root: &Path, requested: &[PathBuf]) -> Result<WorkflowSet, String> {
    let mut set = WorkflowSet {
        features: vec![],
        inputs: BTreeMap::new(),
        diagnostics: vec![],
        discovered: requested.is_empty(),
    };
    let paths = if requested.is_empty() {
        discover(root)?
    } else {
        requested.to_vec()
    };
    let mut names = BTreeSet::new();
    let mut classes = BTreeSet::new();
    for path in paths {
        let relative = if path.is_absolute() {
            compiler::slash(
                path.strip_prefix(root)
                    .map_err(|_| "Workflow must be inside --root")?,
            )
        } else {
            compiler::slash(&path)
        };
        if !relative.ends_with(".workflow.vitrifi") {
            return Err("Workflow files must end in .workflow.vitrifi".into());
        }
        if set.inputs.contains_key(&relative) {
            continue;
        }
        let source = compiler::read_inside(root, &relative)?;
        set.inputs
            .insert(relative.clone(), compiler::hash_bytes(source.as_bytes()));
        match workflow::parse(&relative, &source) {
            Err(errors) => set.diagnostics.extend(errors),
            Ok(feature) => {
                set.diagnostics.extend(workflow::lint(&feature));
                if !names.insert(feature.feature.clone()) || !classes.insert(feature.emit.clone()) {
                    set.diagnostics.push(Diagnostic {
                        code: "WORKFLOW_DUPLICATE".into(),
                        message: "Workflow features and emitted classes must be unique".into(),
                        location: feature.location.clone(),
                    });
                }
                set.features.push(feature);
            }
        }
    }
    Ok(set)
}

fn diagnostics(errors: &[Diagnostic]) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(serde_json::to_string_pretty(errors).unwrap())
    }
}

/// Check every existing path component before opening or creating output.
fn safe_output(root: &Path, path: &str) -> Result<PathBuf, String> {
    let mut destination = root.to_path_buf();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => destination.push(name),
            _ => return Err("Unsafe generated output path".into()),
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "Symlinked workflow output: {}",
                    destination.display()
                ))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(destination)
}

impl WorkflowSet {
    pub fn current(&self, root: &Path) -> bool {
        (!self.discovered
            || discover(root).is_ok_and(|paths| {
                paths
                    .into_iter()
                    .map(|p| compiler::slash(&p))
                    .collect::<BTreeSet<_>>()
                    == self.inputs.keys().cloned().collect()
            }))
            && self.inputs.iter().all(|(path, hash)| {
                compiler::read_inside(root, path)
                    .is_ok_and(|s| compiler::hash_bytes(s.as_bytes()) == *hash)
            })
    }
    pub fn outputs(
        &self,
        root: &Path,
        options: &java::Options,
    ) -> Result<BTreeMap<String, String>, String> {
        diagnostics(&self.diagnostics)?;
        let source = if options.source_root.is_absolute() {
            options
                .source_root
                .strip_prefix(root)
                .map_err(|_| "Source root must be inside --root")?
        } else {
            &options.source_root
        };
        let mut outputs = BTreeMap::new();
        for feature in &self.features {
            let path =
                compiler::slash(&source.join(format!("{}.java", feature.emit.replace('.', "/"))));
            safe_output(root, &path)?;
            outputs.insert(
                path,
                workflow::render(feature).map_err(|e| serde_json::to_string(&e).unwrap())?,
            );
        }
        Ok(outputs)
    }
    pub fn fresh(&self, root: &Path, options: &java::Options) -> Result<(), String> {
        let outputs = self.outputs(root, options)?;
        let source_root = if options.source_root.is_absolute() {
            options.source_root.clone()
        } else {
            root.join(&options.source_root)
        };
        for path in compiler::repository_files(root)? {
            if path.ends_with(".java")
                && root.join(&path).starts_with(&source_root)
                && compiler::read_inside(root, &path)?.starts_with(workflow::GENERATED_MARKER)
                && !outputs.contains_key(&path)
            {
                return Err(format!("WORKFLOW_ORPHAN: {path} has no selected workflow; restore its workflow or explicitly remove the obsolete generated class"));
            }
        }
        for (path, expected) in outputs {
            if compiler::read_inside(root, &path).ok().as_ref() != Some(&expected) {
                return Err(format!(
                    "WORKFLOW_STALE: {path} differs from its workflow; run vitrifi workflow build"
                ));
            }
        }
        Ok(())
    }
    pub fn validate(&self, analysis: &java::Analysis) -> Result<(), String> {
        diagnostics(&self.diagnostics)?;
        if !self.features.is_empty() {
            diagnostics(&crate::derived::scoped_diagnostics(
                &analysis.program,
                &BTreeSet::new(),
            ))?;
        }
        for feature in &self.features {
            diagnostics(&workflow::check_leaves(feature, &analysis.program))?;
        }
        Ok(())
    }
    pub fn build(
        &self,
        root: &Path,
        options: &java::Options,
        target_paths: &[PathBuf],
    ) -> Result<Vec<String>, String> {
        let outputs = self.outputs(root, options)?;
        if outputs.is_empty() {
            return Err("No .workflow.vitrifi files found".into());
        }
        // Ownership and original bytes are checked before analysis and again before writes.
        let mut originals = BTreeMap::new();
        for (path, expected) in &outputs {
            let destination = safe_output(root, path)?;
            let original = if destination.exists() {
                Some(compiler::read_inside(root, path)?)
            } else {
                None
            };
            if let Some(old) = &original {
                let header = expected.lines().take(2).collect::<Vec<_>>().join("\n") + "\n";
                if !old.starts_with(&header) {
                    return Err(format!(
                        "Refusing to overwrite authored or differently owned Java: {path}"
                    ));
                }
            }
            originals.insert(path.clone(), original);
        }
        let analysis = java::analyze_compiled(root, options, &outputs)?;
        self.validate(&analysis)?;
        let target_set = if !target_paths.is_empty() || !targets::discover(root)?.is_empty() {
            Some(targets::load(root, target_paths)?)
        } else {
            None
        };
        if let Some(set) = &target_set {
            diagnostics(&set.diagnostics)?;
            let report = target::conform(&set.features, &analysis.program);
            if report.status != "passed" {
                return Err(serde_json::to_string_pretty(&report).unwrap());
            }
        }
        // Also check generated operation attribution, including Java compilation errors.
        diagnostics(&analysis.program.diagnostics)?;
        if !self.current(root)
            || !java::inputs_current(root, &analysis.inputs)
            || target_set.as_ref().is_some_and(|s| !s.current(root))
        {
            return Err("Workflow, Java, target, or dependency changed during build; retry".into());
        }
        for (path, before) in &originals {
            safe_output(root, path)?;
            if &compiler::read_inside(root, path).ok() != before {
                return Err("Generated output changed during build; retry".into());
            }
        }
        for (path, text) in &outputs {
            if originals[path].as_ref() == Some(text) {
                continue;
            }
            let destination = safe_output(root, path)?;
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
            let mut temporary = tempfile::NamedTempFile::new_in(destination.parent().unwrap())
                .map_err(|e| e.to_string())?;
            temporary
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
            temporary.persist(destination).map_err(|e| e.to_string())?;
        }
        Ok(outputs.into_keys().collect())
    }
}

/// Workflows are executable authored code; surface their source changes separately.
pub fn review_changes(root: &Path, base: &str) -> Result<Vec<serde_json::Value>, String> {
    targets::review_file_changes(root, base, ".workflow.vitrifi", "authored_workflow",
        "Review executable workflow changes alongside Java leaves and targets; equal effect sets do not imply equal behavior.")
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLE: &str = include_str!("../examples/workflow-checkout/src/main/java/dev/vitrifi/example/checkout/Checkout.Discounts.workflow.vitrifi");
    fn options() -> java::Options {
        java::Options {
            analyzer: "missing.jar".into(),
            classpath_file: "missing.txt".into(),
            source_root: "src/main/java".into(),
            java: "missing-java".into(),
        }
    }
    #[test]
    fn discovery_identity_and_duplicate_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("A.workflow.vitrifi"), EXAMPLE).unwrap();
        let set = load(&root, &[]).unwrap();
        assert!(set.diagnostics.is_empty());
        assert!(set.current(&root));
        fs::write(root.join("B.workflow.vitrifi"), EXAMPLE).unwrap();
        assert!(!set.current(&root));
        assert!(load(&root, &[])
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == "WORKFLOW_DUPLICATE"));
    }
    #[test]
    fn freshness_and_authored_output_protection_need_no_java_execution() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("A.workflow.vitrifi"), EXAMPLE).unwrap();
        let set = load(&root, &[]).unwrap();
        let options = options();
        assert!(set
            .fresh(&root, &options)
            .unwrap_err()
            .contains("WORKFLOW_STALE"));
        let outputs = set.outputs(&root, &options).unwrap();
        let (path, text) = outputs.iter().next().unwrap();
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        fs::write(root.join(path), "authored Java").unwrap();
        assert!(set
            .build(&root, &options, &[])
            .unwrap_err()
            .contains("Refusing to overwrite"));
        assert_eq!(
            fs::read_to_string(root.join(path)).unwrap(),
            "authored Java"
        );
        fs::write(root.join(path), text).unwrap();
        set.fresh(&root, &options).unwrap();
        fs::remove_file(root.join("A.workflow.vitrifi")).unwrap();
        assert!(load(&root, &[])
            .unwrap()
            .fresh(&root, &options)
            .unwrap_err()
            .contains("WORKFLOW_ORPHAN"));
    }
    #[test]
    fn output_paths_reject_traversal_and_absolute_paths() {
        let temp = tempfile::tempdir().unwrap();
        for path in ["../escape.java", "/outside.java", "src/../../outside.java"] {
            assert!(safe_output(temp.path(), path).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlink_inputs_and_output_ancestors_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("real"), EXAMPLE).unwrap();
        std::os::unix::fs::symlink("real", root.join("A.workflow.vitrifi")).unwrap();
        assert!(load(&root, &["A.workflow.vitrifi".into()]).is_err());
        std::os::unix::fs::symlink(temp.path(), root.join("src")).unwrap();
        assert!(safe_output(&root, "src/main/java/X.java").is_err());
    }
}
