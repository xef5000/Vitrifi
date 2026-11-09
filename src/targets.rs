//! File loading and input identity for independently authored architecture targets.
use crate::{compiler, model::Diagnostic, target};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct TargetSet {
    pub features: Vec<target::TargetFeature>,
    pub files: Vec<String>,
    pub revision: String,
    pub inputs: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    discovered: bool,
}

pub fn discover(root: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(compiler::repository_files(root)?
        .into_iter()
        .filter(|path| path.ends_with(".target.vitrifi"))
        .map(PathBuf::from)
        .collect())
}

pub fn load(root: &Path, requested: &[PathBuf]) -> Result<TargetSet, String> {
    let paths = if requested.is_empty() {
        discover(root)?
    } else {
        requested.to_vec()
    };
    if paths.is_empty() {
        return Err(
            "No architecture targets found; pass --target path/to/Feature.target.vitrifi".into(),
        );
    }
    let mut files = BTreeSet::new();
    let mut inputs = BTreeMap::new();
    let mut features = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let absolute = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        // read_inside rejects path traversal, symlinked files/directories and oversized input.
        let relative = compiler::slash(
            absolute
                .strip_prefix(root)
                .map_err(|_| "Architecture targets must be inside --root")?,
        );
        if !relative.ends_with(".target.vitrifi") {
            return Err("Architecture target files must end in .target.vitrifi".into());
        }
        if !files.insert(relative.clone()) {
            continue;
        }
        let source = compiler::read_inside(root, &relative)?;
        inputs.insert(relative.clone(), compiler::hash_bytes(source.as_bytes()));
        match target::parse(&relative, &source) {
            Ok(feature) => features.push(feature),
            Err(errors) => diagnostics.extend(errors),
        }
    }
    diagnostics.extend(target::lint(&features));
    Ok(TargetSet {
        revision: compiler::content_digest(&inputs),
        files: files.into_iter().collect(),
        features,
        inputs,
        diagnostics,
        discovered: requested.is_empty(),
    })
}

impl TargetSet {
    pub fn current(&self, root: &Path) -> bool {
        let selection_current = !self.discovered
            || discover(root).is_ok_and(|paths| {
                paths
                    .into_iter()
                    .map(|p| compiler::slash(&p))
                    .collect::<BTreeSet<_>>()
                    == self.files.iter().cloned().collect()
            });
        selection_current
            && self.inputs.iter().all(|(path, hash)| {
                compiler::read_inside(root, path)
                    .is_ok_and(|source| compiler::hash_bytes(source.as_bytes()) == *hash)
            })
    }
}

/// Review authored target changes independently of implementation facts.
/// Historical targets are read as data; no historical build or test is executed.
pub fn review_changes(root: &Path, base: &str) -> Result<Vec<Value>, String> {
    review_file_changes(
        root,
        base,
        ".target.vitrifi",
        "authored_target",
        "Review this target source diff separately; conformance does not approve policy changes.",
    )
}

pub(crate) fn review_file_changes(
    root: &Path,
    base: &str,
    suffix: &str,
    provenance: &str,
    message: &str,
) -> Result<Vec<Value>, String> {
    fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
        let output = std::process::Command::new("git")
            .args(["-c", "core.fsmonitor=false", "-C"])
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().into())
        }
    }
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
    .to_owned();
    let mut before = BTreeMap::new();
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
        if let Some(path) = relative.filter(|p| p.ends_with(suffix)) {
            before.insert(
                path.to_owned(),
                compiler::hash_bytes(&git(&top, &["show", &format!("{revision}:{full}")])?),
            );
        }
    }
    let mut after = BTreeMap::new();
    for path in compiler::repository_files(root)?
        .into_iter()
        .filter(|p| p.ends_with(suffix))
    {
        after.insert(
            path.clone(),
            compiler::hash_bytes(compiler::read_inside(root, &path)?.as_bytes()),
        );
    }
    let mut changes = Vec::new();
    for path in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        if before.get(path) != after.get(path) {
            changes.push(json!({"path":path,"change":if !before.contains_key(path){"added"}else if !after.contains_key(path){"removed"}else{"modified"},
                "before_sha256":before.get(path),"after_sha256":after.get(path),"provenance":provenance,
                "message":message}));
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git(root: &Path, arguments: &[&str]) {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "user.name=Vitrifi Test",
                "-c",
                "user.email=vitrifi@example.invalid",
                "-C",
            ])
            .arg(root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let destination = root.join(path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, contents).unwrap();
    }

    #[test]
    fn review_reports_target_additions_modifications_and_removals_independently() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        write(&root, "Changed.target.vitrifi", "approved target\n");
        write(&root, "Removed.target.vitrifi", "removed target\n");
        write(&root, "Unchanged.target.vitrifi", "retained target\n");
        write(
            &root,
            "Changed.vitrifi",
            "// @generated by vitrifi source analysis\nold snapshot\n",
        );
        write(&root, "Removed.vitrifi", "old generated snapshot\n");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "baseline"]);

        write(&root, "Changed.vitrifi", "new generated snapshot\n");
        write(&root, "Added.vitrifi", "new generated snapshot\n");
        fs::remove_file(root.join("Removed.vitrifi")).unwrap();
        assert!(review_changes(&root, "HEAD").unwrap().is_empty());

        write(&root, "Added.target.vitrifi", "new target\n");
        write(&root, "Changed.target.vitrifi", "proposed target\n");
        fs::remove_file(root.join("Removed.target.vitrifi")).unwrap();
        let changes = review_changes(&root, "HEAD").unwrap();
        assert_eq!(changes.len(), 3, "{changes:?}");
        for (change, path, kind, before, after) in [
            (
                &changes[0],
                "Added.target.vitrifi",
                "added",
                None,
                Some("new target\n"),
            ),
            (
                &changes[1],
                "Changed.target.vitrifi",
                "modified",
                Some("approved target\n"),
                Some("proposed target\n"),
            ),
            (
                &changes[2],
                "Removed.target.vitrifi",
                "removed",
                Some("removed target\n"),
                None,
            ),
        ] {
            assert_eq!(change["path"], path);
            assert_eq!(change["change"], kind);
            assert_eq!(change["provenance"], "authored_target");
            assert_eq!(
                change["before_sha256"],
                json!(before.map(|source| compiler::hash_bytes(source.as_bytes())))
            );
            assert_eq!(
                change["after_sha256"],
                json!(after.map(|source| compiler::hash_bytes(source.as_bytes())))
            );
        }
        assert!(review_changes(&root, "does-not-exist").is_err());
    }

    #[test]
    fn review_scopes_targets_to_the_selected_application_and_preserves_relative_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        write(
            &root,
            "apps/checkout/features/Checkout.target.vitrifi",
            "approved target\n",
        );
        write(
            &root,
            "apps/checkout-other/Other.target.vitrifi",
            "neighbor\n",
        );
        write(&root, "Outside.target.vitrifi", "outside\n");
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "baseline"]);
        write(
            &root,
            "apps/checkout/features/Checkout.target.vitrifi",
            "changed target\n",
        );
        write(
            &root,
            "apps/checkout/features/New feature.target.vitrifi",
            "new target\n",
        );
        write(
            &root,
            "apps/checkout/features/Checkout.vitrifi",
            "generated\n",
        );
        write(
            &root,
            "apps/checkout-other/Other.target.vitrifi",
            "changed neighbor\n",
        );
        fs::remove_file(root.join("Outside.target.vitrifi")).unwrap();

        let changes = review_changes(&root.join("apps/checkout"), "HEAD").unwrap();
        let paths: Vec<_> = changes
            .iter()
            .map(|change| change["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "features/Checkout.target.vitrifi",
                "features/New feature.target.vitrifi"
            ]
        );
        assert_eq!(changes[0]["change"], "modified");
        assert_eq!(changes[1]["change"], "added");
    }

    #[test]
    fn discovery_is_explicit_and_does_not_execute_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(
            root.join("View.vitrifi"),
            "// @generated by vitrifi source analysis",
        )
        .unwrap();
        assert!(discover(&root).unwrap().is_empty());
        assert!(load(&root, &[]).is_err());
        fs::write(root.join("Broken.target.vitrifi"), "target feature {").unwrap();
        let targets = load(&root, &[]).unwrap();
        assert_eq!(targets.files.len(), 1);
        assert!(!targets.diagnostics.is_empty());
        assert!(targets.current(&root));
        fs::write(root.join("Added.target.vitrifi"), "target feature Added {}").unwrap();
        assert!(!targets.current(&root));
        let explicit = load(&root, &["Broken.target.vitrifi".into()]).unwrap();
        assert!(explicit.current(&root));
        fs::write(root.join("Broken.target.vitrifi"), "changed").unwrap();
        assert!(!targets.current(&root));
        assert!(!explicit.current(&root));
    }

    #[cfg(unix)]
    #[test]
    fn explicit_symlink_targets_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("source"), "target feature X {}").unwrap();
        std::os::unix::fs::symlink("source", root.join("X.target.vitrifi")).unwrap();
        assert!(load(&root, &["X.target.vitrifi".into()]).is_err());
    }
}
