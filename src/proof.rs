//! Certificate format and orchestration. The kernel does not depend on this
//! producer/IO layer and never accepts a recorded success flag as evidence.
use crate::{
    compiler, java, proof_context as context, proof_kernel as kernel, proof_search, workflow,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const SCHEMA: &str = "vitrifi.proof/1";

/// Pin implementation as well as semantic version names. A modified parser,
/// checker, lowering or Java-input interpreter invalidates prior certificates.
pub fn implementation() -> String {
    compiler::hash_bytes(
        concat!(
            env!("CARGO_PKG_VERSION"),
            include_str!("proof.rs"),
            include_str!("proof_kernel.rs"),
            include_str!("proof_context.rs"),
            include_str!("main.rs"),
            include_str!("lib.rs"),
            include_str!("workflow.rs"),
            include_str!("workflows.rs"),
            include_str!("target.rs"),
            include_str!("targets.rs"),
            include_str!("java.rs"),
            include_str!("derived.rs"),
            include_str!("derived_model.rs"),
            include_str!("compiler.rs"),
            include_str!("model.rs"),
            include_str!("../Cargo.lock")
        )
        .as_bytes(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Certificate {
    pub schema: String,
    pub semantics: String,
    pub rules: String,
    pub implementation_sha256: String,
    pub base_commit: String,
    pub workflow: String,
    pub before_sha256: String,
    pub after_sha256: String,
    pub context: context::Context,
    pub before_steps: Vec<kernel::Step>,
    pub after_steps: Vec<kernel::Step>,
    pub common_term_sha256: String,
}

fn versions(certificate: &Certificate) -> Result<(), String> {
    if certificate.schema != SCHEMA
        || certificate.semantics != workflow::SEMANTICS
        || certificate.rules != kernel::RULES
        || certificate.implementation_sha256 != implementation()
    {
        return Err(
            "PROOF_VERSION: unsupported schema, semantics, laws or checker implementation".into(),
        );
    }
    if certificate.base_commit.len() != 40 && certificate.base_commit.len() != 64
        || !certificate
            .base_commit
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("PROOF_BASELINE: certificate must name an immutable full Git commit".into());
    }
    Ok(())
}

pub fn produce(
    root: &Path,
    workflow: &Path,
    base: &str,
    options: &java::Options,
    targets: &[PathBuf],
) -> Result<Certificate, String> {
    let pair = context::load_pair(root, workflow, base)?;
    let (before_steps, after_steps, common_term_sha256) =
        proof_search::derive(&pair.before, &pair.after)?;
    let context = context::capture(root, &pair, options, targets)?;
    Ok(Certificate {
        schema: SCHEMA.into(),
        semantics: workflow::SEMANTICS.into(),
        rules: kernel::RULES.into(),
        implementation_sha256: implementation(),
        base_commit: pair.commit,
        workflow: pair.path,
        before_sha256: compiler::hash_bytes(pair.before_source.as_bytes()),
        after_sha256: compiler::hash_bytes(pair.after_source.as_bytes()),
        context,
        before_steps,
        after_steps,
        common_term_sha256,
    })
}

/// Does not call proof search. Load actual endpoints, replay each law and
/// reconstruct every contextual assumption from the selected project/toolchain.
pub fn check(
    root: &Path,
    certificate: &Certificate,
    expected_base: Option<&str>,
    options: &java::Options,
    targets: &[PathBuf],
) -> Result<(), String> {
    versions(certificate)?;
    if let Some(reference) = expected_base {
        if context::resolve(root, reference)? != certificate.base_commit {
            return Err(
                "PROOF_BASELINE: certificate does not cover the required base commit".into(),
            );
        }
    }
    let pair = context::load_pair(
        root,
        Path::new(&certificate.workflow),
        &certificate.base_commit,
    )?;
    if compiler::hash_bytes(pair.before_source.as_bytes()) != certificate.before_sha256
        || compiler::hash_bytes(pair.after_source.as_bytes()) != certificate.after_sha256
    {
        return Err("PROOF_STALE: workflow source hashes do not match the actual endpoints".into());
    }
    kernel::check(
        &pair.before,
        &pair.after,
        &certificate.before_steps,
        &certificate.after_steps,
        &certificate.common_term_sha256,
    )?;
    let actual = context::capture(root, &pair, options, targets)?;
    if actual != certificate.context {
        return Err("PROOF_CONTEXT_STALE: native inputs, target selection, dependencies, analyzer or Java runtime changed".into());
    }
    Ok(())
}

pub fn read(root: &Path, path: &Path) -> Result<Certificate, String> {
    let relative = context::relative(root, path)?;
    if !relative.ends_with(".proof.json") {
        return Err("PROOF_PATH: certificate must end in .proof.json".into());
    }
    serde_json::from_str(&compiler::read_inside(root, &relative)?)
        .map_err(|e| format!("PROOF_FORMAT: {e}"))
}
pub fn write(root: &Path, path: &Path, certificate: &Certificate) -> Result<(), String> {
    let relative = context::relative(root, path)?;
    if !relative.ends_with(".proof.json") {
        return Err("PROOF_PATH: certificate must end in .proof.json".into());
    }
    let destination = root.join(relative);
    let parent = destination
        .parent()
        .ok_or("PROOF_PATH: missing parent directory")?;
    // Require an existing ordinary directory and never replace an existing file.
    if parent.canonicalize().map_err(|e| e.to_string())? != parent {
        return Err("PROOF_PATH: symlinked output directory".into());
    }
    if fs::symlink_metadata(&destination).is_ok() {
        return Err("PROOF_OUTPUT_EXISTS: choose a new certificate path or remove the old artifact explicitly".into());
    }
    let bytes = serde_json::to_vec_pretty(certificate).unwrap();
    if bytes.len() >= 2_000_000 {
        return Err("PROOF_LIMIT: certificate exceeds 2 MB".into());
    }
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary.write_all(b"\n").map_err(|e| e.to_string())?;
    temporary
        .persist_noclobber(destination)
        .map_err(|e| e.to_string())?;
    Ok(())
}
