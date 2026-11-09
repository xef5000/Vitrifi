//! Structural declaration changes for ordinary Git pull-request review.
use crate::{
    compiler::{read_inside, repository_files, slash},
    parser,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
};

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let result = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().into());
    }
    Ok(result.stdout)
}
fn semantic(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("location");
            for value in map.values_mut() {
                semantic(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                semantic(value);
            }
        }
        _ => (),
    }
}
fn fields(path: &str, source: Option<&str>) -> Result<BTreeMap<String, Value>, String> {
    let Some(source) = source else {
        return Ok(BTreeMap::new());
    };
    let module = parser::parse(path, source).map_err(|e| serde_json::to_string(&e).unwrap())?;
    let mut entries = BTreeMap::from([
        ("feature".into(), json!(module.name)),
        ("purpose".into(), json!(module.purpose)),
        ("questions".into(), json!(module.questions)),
        ("imports".into(), json!(module.imports)),
    ]);
    for declaration in module.declarations {
        entries.insert(
            format!("{} {}", declaration.body.kind(), declaration.name),
            json!(declaration),
        );
    }
    for value in entries.values_mut() {
        semantic(value);
    }
    Ok(entries)
}
pub fn review(root: &Path, base: &str, entry: Option<&Path>) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let top_text = String::from_utf8(git(&root, &["rev-parse", "--show-toplevel"])?)
        .map_err(|e| e.to_string())?;
    let top = Path::new(top_text.trim())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let prefix = slash(root.strip_prefix(&top).map_err(|e| e.to_string())?);
    let revision = String::from_utf8(git(
        &root,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    )?)
    .map_err(|e| e.to_string())?
    .trim()
    .to_string();
    let mut old_paths = BTreeMap::new();
    for raw in git(&top, &["ls-tree", "-r", "--name-only", "-z", &revision])?
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
    {
        let full = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
        let path = if prefix.is_empty() {
            Some(full)
        } else {
            full.strip_prefix(&format!("{prefix}/"))
        };
        if let Some(path) = path {
            if path.ends_with(".vitrifi")
                && !path.ends_with(".target.vitrifi")
                && !path.ends_with(".workflow.vitrifi")
            {
                old_paths.insert(path.to_string(), full.to_string());
            }
        }
    }
    let current: BTreeSet<_> = repository_files(&root)?
        .into_iter()
        .filter(|p| {
            p.ends_with(".vitrifi")
                && !p.ends_with(".target.vitrifi")
                && !p.ends_with(".workflow.vitrifi")
        })
        .collect();
    let mut paths: BTreeSet<String> = current
        .union(&old_paths.keys().cloned().collect())
        .cloned()
        .collect();
    if let Some(entry) = entry {
        let path = if entry.is_absolute() {
            slash(
                entry
                    .strip_prefix(&root)
                    .map_err(|_| "Entry outside root")?,
            )
        } else {
            slash(entry)
        };
        let path = crate::compiler::relative_path("entry.vitrifi", &path)?;
        paths.retain(|p| *p == path);
    }
    let mut changes = vec![];
    for path in paths {
        let old = if let Some(full) = old_paths.get(&path) {
            Some(
                String::from_utf8(git(&top, &["show", &format!("{revision}:{full}")])?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let new = if current.contains(&path) {
            Some(read_inside(&root, &path)?)
        } else {
            None
        };
        let before = fields(&path, old.as_deref())?;
        let after = fields(&path, new.as_deref())?;
        for key in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
            if before.get(key) == after.get(key) {
                continue;
            }
            changes.push(json!({"file":path,"declaration":key,"change":if !before.contains_key(key){"added"}else if !after.contains_key(key){"removed"}else{"modified"},"before":before.get(key),"after":after.get(key)}));
        }
    }
    Ok(
        json!({"base":revision,"changes":changes,"scope":"Authored .vitrifi declarations compared with a Git commit, including untracked feature files. Locations and formatting are ignored. Implementation changes still require ordinary code review."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_review_ignores_source_positions() {
        assert_eq!(
            fields("a.vitrifi", Some("feature A { resource C; } ")).unwrap(),
            fields(
                "a.vitrifi",
                Some("// comment\nfeature A {\n resource C;\n}\n")
            )
            .unwrap()
        );
    }
    #[test]
    fn semantic_review_preserves_export_and_type_changes() {
        assert_ne!(
            fields("a.vitrifi", Some("feature A { resource C; }")).unwrap(),
            fields("a.vitrifi", Some("feature A { export resource C; }")).unwrap()
        );
        assert_ne!(
            fields("a.vitrifi", Some("feature A { resource C; }")).unwrap(),
            fields("a.vitrifi", Some("feature A { type C; }")).unwrap()
        );
    }
}
