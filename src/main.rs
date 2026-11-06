use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use vitrifi::{compiler, evidence, java, model::*, review, target, targets, workflows};

const HELP:&str="Vitrifi — a compiled architecture language\n\nUsage: vitrifi <command> [feature.vitrifi] [--root DIR] [--json]\n\n  init       Create a feature-local source file\n  check      Compile and check a feature and its imported contracts (or all features)\n  compile    Emit the linked graph to .vitrifi/program.json (or --out FILE)\n  inspect    Read typed feature contracts in the terminal\n  source     Expand a bound implementation: source feature.vitrifi operation\n  review     Compare architecture declarations with a Git commit: --base REF\n  verify     Explicitly run declared verification commands\n  evidence   Inspect recorded evidence and detect stale inputs\n\nAll paths inside feature files are relative to that feature file.\nCompilation never executes feature source or verification commands.\n";

#[derive(Default)]
struct Args {
    command: String,
    root: PathBuf,
    entry: Option<PathBuf>,
    member: Option<String>,
    json: bool,
    out: Option<PathBuf>,
    base: Option<String>,
    java: bool,
    check_generated: bool,
    analyzer: Option<PathBuf>,
    classpath_file: Option<PathBuf>,
    source_root: Option<PathBuf>,
    runner: Vec<String>,
    timeout: Option<u64>,
    targets: Vec<PathBuf>,
}
fn arguments() -> Result<Args, String> {
    let mut args = Args {
        root: env::current_dir().map_err(|e| e.to_string())?,
        ..Args::default()
    };
    let mut it = env::args().skip(1);
    while let Some(value) = it.next() {
        match value.as_str() {
            "--" => {
                args.runner = it.collect();
                break;
            }
            "--java" => args.java = true,
            "--target" => args.targets.push(
                it.next()
                    .ok_or("--target requires a .target.vitrifi path")?
                    .into(),
            ),
            "--check" => args.check_generated = true,
            "--java-analyzer" => {
                args.analyzer = Some(
                    it.next()
                        .ok_or("--java-analyzer requires a JAR path")?
                        .into(),
                )
            }
            "--classpath-file" => {
                args.classpath_file =
                    Some(it.next().ok_or("--classpath-file requires a file")?.into())
            }
            "--source-root" => {
                args.source_root = Some(
                    it.next()
                        .ok_or("--source-root requires a directory")?
                        .into(),
                )
            }
            "--timeout" => {
                args.timeout = Some(
                    it.next()
                        .ok_or("--timeout requires seconds")?
                        .parse()
                        .map_err(|_| "Invalid --timeout")?,
                )
            }
            "--json" => args.json = true,
            "--root" => args.root = PathBuf::from(it.next().ok_or("--root requires a directory")?),
            "--out" => args.out = Some(PathBuf::from(it.next().ok_or("--out requires a file")?)),
            "--base" => args.base = Some(it.next().ok_or("--base requires a Git revision")?),
            "--help" | "-h" => {
                args.command = "help".into();
                return Ok(args);
            }
            "--version" => {
                args.command = "version".into();
                return Ok(args);
            }
            _ if value.starts_with('-') => return Err(format!("Unknown option {value}")),
            _ if args.command.is_empty() => args.command = value,
            _ if args.entry.is_none() => args.entry = Some(value.into()),
            _ if args.member.is_none() => args.member = Some(value),
            _ => return Err("Too many arguments".into()),
        }
    }
    if args.command.is_empty() {
        args.command = "help".into();
    }
    Ok(args)
}
fn print_diagnostics(program: &Program) {
    for d in &program.diagnostics {
        println!(
            "{}:{}:{}: error[{}]: {}",
            d.location.path, d.location.line, d.location.column, d.code, d.message
        );
    }
}
fn type_name(ty: &TypeExpr) -> String {
    if ty.arguments.is_empty() {
        ty.name.clone()
    } else {
        format!(
            "{}<{}>",
            ty.name,
            ty.arguments
                .iter()
                .map(type_name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}
fn inspect(program: &Program) {
    for module in &program.modules {
        println!(
            "feature {}  ({})\n  {}",
            module.name, module.path, module.purpose
        );
        for import in &module.imports {
            println!("  use {} as {}", import.path, import.alias);
        }
        for declaration in &module.declarations {
            let public = if declaration.public { "export " } else { "" };
            match &declaration.body {
                DeclBody::Operation {
                    parameters,
                    returns,
                    binding,
                    effects,
                    calls,
                    requires,
                    ensures,
                    ..
                } => {
                    println!(
                        "  {public}operation {}({}) -> {}",
                        declaration.name,
                        parameters
                            .iter()
                            .map(|p| format!("{}: {}", p.name, type_name(&p.ty)))
                            .collect::<Vec<_>>()
                            .join(", "),
                        type_name(returns)
                    );
                    println!(
                        "    binds {} {}::{}",
                        binding.language, binding.path, binding.symbol
                    );
                    println!(
                        "    effects [{}]",
                        effects
                            .iter()
                            .map(|e| format!("{} {}", e.action, e.resource.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    for (label, refs) in [
                        ("calls", calls),
                        ("requires", requires),
                        ("ensures", ensures),
                    ] {
                        if !refs.is_empty() {
                            println!(
                                "    {label} [{}]",
                                refs.iter()
                                    .map(|r| r.name.clone())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            );
                        }
                    }
                }
                DeclBody::Requirement { text } => println!(
                    "  {public}requirement {}: {text} [declared]",
                    declaration.name
                ),
                DeclBody::Boundary { paths, allows } => println!(
                    "  {public}boundary {} {:?} allows [{}]",
                    declaration.name,
                    paths,
                    allows
                        .iter()
                        .map(|r| r.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                DeclBody::Check { command, .. } => {
                    println!("  check {} {:?} [not executed]", declaration.name, command)
                }
                other => println!("  {public}{} {}", other.kind(), declaration.name),
            }
        }
        for question in &module.questions {
            println!("  question: {question}");
        }
        println!();
    }
    print_diagnostics(program);
}
fn output(_args: &Args, value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

fn java_options(args: &Args) -> Result<java::Options, String> {
    let analyzer = args
        .analyzer
        .clone()
        .or_else(|| env::var_os("VITRIFI_JAVA_ANALYZER").map(PathBuf::from))
        .unwrap_or(env::current_dir().map_err(|e| e.to_string())?.join(format!(
            "integrations/java/target/vitrifi-java-{}.jar",
            env!("CARGO_PKG_VERSION")
        )));
    let java = env::var_os("VITRIFI_JAVA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("JAVA_HOME").map(|p| PathBuf::from(p).join("bin/java")))
        .unwrap_or_else(|| PathBuf::from("java"));
    Ok(java::Options {
        analyzer,
        java,
        classpath_file: args
            .classpath_file
            .clone()
            .unwrap_or_else(|| "target/vitrifi-classpath.txt".into()),
        source_root: args
            .source_root
            .clone()
            .unwrap_or_else(|| "src/main/java".into()),
    })
}

fn generated_entry(root: &Path, entry: Option<&Path>) -> Result<bool, String> {
    let paths = if let Some(entry) = entry {
        vec![if entry.is_absolute() {
            entry.to_path_buf()
        } else {
            root.join(entry)
        }]
    } else {
        compiler::repository_files(root)?
            .into_iter()
            .filter(|p| p.ends_with(".vitrifi"))
            .map(|p| root.join(p))
            .collect()
    };
    Ok(paths.into_iter().any(|p| {
        fs::read_to_string(p).is_ok_and(|s| s.starts_with(vitrifi::derived::GENERATED_MARKER))
    }))
}

fn evidence_selection(args: &Args) -> java::EvidenceSelection {
    java::EvidenceSelection {
        source_root: args.source_root.clone(),
        analyzer: args
            .analyzer
            .clone()
            .or_else(|| env::var_os("VITRIFI_JAVA_ANALYZER").map(PathBuf::from)),
        classpath_file: args.classpath_file.clone(),
    }
}

fn stale_target_selection() -> Value {
    json!({"status":"stale","fresh":false,"executed":false,
        "message":"Discovered target selection changed during this command; retry to check the new targets"})
}

fn execute_workflows(args: &Args, root: &Path) -> Result<u8, String> {
    if args.check_generated || args.out.is_some() || args.base.is_some() {
        return Err("Workflow commands do not accept --check, --out, or --base; use workflow check for validation without writes".into());
    }
    let command = args
        .entry
        .as_ref()
        .and_then(|p| p.to_str())
        .unwrap_or("check");
    if !matches!(command, "lint" | "build" | "check" | "verify") {
        return Err(
            "Usage: vitrifi workflow lint|build|check|verify [FILE.workflow.vitrifi] --root DIR"
                .into(),
        );
    }
    if command != "verify" && !args.runner.is_empty() {
        return Err("Only workflow verify accepts a command after --".into());
    }
    if command == "lint" && !args.targets.is_empty() {
        return Err("Use lint --target to lint targets, or workflow check --target to check their implementation".into());
    }
    let requested: Vec<PathBuf> = args.member.iter().map(PathBuf::from).collect();
    let set = workflows::load(root, &requested)?;
    if set.inputs.is_empty() {
        return Err("No .workflow.vitrifi files found".into());
    }
    if !set.diagnostics.is_empty() || command == "lint" {
        let passed = set.diagnostics.is_empty();
        output(
            args,
            &json!({"status":if passed {"passed"} else {"failed"}, "diagnostics":set.diagnostics, "features":set.features,
            "files":set.inputs.keys().collect::<Vec<_>>(), "scope":"Workflow syntax, types, modeled effect bounds and explicit rejection paths; Java actions not checked."}),
        );
        return Ok(if passed { 0 } else { 1 });
    }
    let options = java_options(args)?;
    if command == "build" {
        let files = set.build(root, &options, &args.targets)?;
        output(args, &json!({"status":"passed", "generated":files}));
        return Ok(0);
    }
    // Include all discovered workflow outputs when checking explicit entries as well.
    let mut all_paths = workflows::discover(root)?;
    all_paths.extend(requested);
    let mut analysis =
        java::analyze_with_workflows(root, &options, workflows::load(root, &all_paths)?)?;
    set.validate(&analysis)?;
    if !set.current(root) {
        return Err("Workflow changed during analysis; retry".into());
    }
    analysis.inputs.extend(set.inputs.clone());
    let target_set = if !args.targets.is_empty() || !targets::discover(root)?.is_empty() {
        Some(targets::load(root, &args.targets)?)
    } else {
        None
    };
    let mut context = None;
    let mut discovered_targets = Vec::new();
    if let Some(targets) = &target_set {
        let conformance = target::conform(&targets.features, &analysis.program);
        if !targets.diagnostics.is_empty() || conformance.status != "passed" {
            output(
                args,
                &json!({"status":"failed", "diagnostics":targets.diagnostics, "conformance":conformance, "executed":false}),
            );
            return Ok(1);
        }
        if !targets.current(root) {
            return Err("Targets changed during analysis; retry".into());
        }
        analysis.inputs.extend(targets.inputs.clone());
        discovered_targets = targets.files.iter().map(PathBuf::from).collect();
        context = Some(java::TargetContext {
            revision: targets.revision.clone(),
            files: targets.files.clone(),
            entries: conformance
                .operations
                .iter()
                .map(|o| java::TargetEntry {
                    feature: o.feature.clone(),
                    method: o.binding.clone(),
                    policies: o.policies.clone(),
                })
                .collect(),
        });
    }
    let mut result = json!({"status":"passed", "files":set.inputs.keys().collect::<Vec<_>>(),
        "scope":"Compiled workflow and attributed Java action bounds; runtime scenarios and native value semantics are separate obligations."});
    if command == "verify" {
        result = java::verify_with_target_selection(
            root,
            &analysis,
            &args.runner,
            args.timeout.unwrap_or(120),
            context.as_ref(),
            args.targets
                .is_empty()
                .then_some(discovered_targets.as_slice()),
        )?;
    }
    output(args, &result);
    Ok(if result["status"] == "passed" { 0 } else { 1 })
}

fn execute_targets(args: &Args, root: &Path, paths: &[PathBuf]) -> Result<u8, String> {
    if !matches!(
        args.command.as_str(),
        "lint" | "conform" | "check" | "inspect" | "compile" | "verify" | "evidence"
    ) {
        return Err(
            "--target is supported by lint, conform, check, inspect, compile, verify, and evidence"
                .into(),
        );
    }
    let unrelated_entry = args
        .entry
        .as_ref()
        .is_some_and(|path| !path.to_string_lossy().ends_with(".target.vitrifi"))
        && !generated_entry(root, args.entry.as_deref())?;
    if args.member.is_some() || unrelated_entry {
        return Err("Target commands accept .target.vitrifi entries; remove the unrelated positional file or member".into());
    }
    let set = targets::load(root, paths)?;
    let identity = json!({"revision":set.revision,"files":set.files});
    if !set.diagnostics.is_empty() || args.command == "lint" {
        let passed = set.diagnostics.is_empty();
        output(
            args,
            &json!({"status":if passed {"passed"} else {"failed"},"target":identity,
            "features":set.features,"diagnostics":set.diagnostics,"scope":"Target syntax and references only; implementation and runtime evidence are not checked by lint."}),
        );
        return Ok(if passed { 0 } else { 1 });
    }
    if args.command == "evidence" {
        let mut result = java::evidence_status_with_selection(root, &evidence_selection(args))?;
        if result["target"]["revision"] != set.revision
            || result["target"]["files"] != json!(set.files)
        {
            result["recorded_status"] = result["status"].clone();
            result["status"] = json!("stale");
            result["fresh"] = json!(false);
            result["target_gap"] = json!("Recorded evidence does not cover the selected target revision and files; run verify --target again");
        }
        let passed = result["status"] == "passed";
        output(args, &result);
        return Ok(if passed { 0 } else { 1 });
    }
    let mut analysis = match java::analyze(root, &java_options(args)?) {
        Ok(analysis) => analysis,
        Err(message) => {
            output(
                args,
                &json!({"status":"failed","target":identity,"diagnostics":[{
                "code":"TARGET_ANALYSIS_FAILED","message":message}],"executed":false}),
            );
            return Ok(1);
        }
    };
    let conformance = target::conform(&set.features, &analysis.program);
    let mut result = json!({"status":conformance.status,"target":identity,"source_revision":analysis.revision,
    "conformance":conformance,"scope":[
        "Native public signatures, resource mappings, modeled effect bounds and source call-target boundaries are compared to authored targets.",
        "Runtime policies still require fresh instrumented scenarios; static conformance does not establish business correctness."
    ]});
    if !set.current(root) {
        result["status"] = json!("stale");
        result["message"] = json!("Target changed during analysis; retry");
    }
    if args.command == "inspect" {
        result["analysis"] = json!(analysis);
        result["targets"] = json!(set.features);
    }
    if result["status"] == "passed" {
        if args.command == "check" {
            let generated = java::generate(root, &analysis, true)?;
            if generated["status"] != "passed" {
                result["status"] = generated["status"].clone();
            }
            result["generated"] = generated;
        } else if args.command == "compile" {
            result["analysis"] = json!(analysis);
        } else if args.command == "verify" {
            analysis.inputs.extend(set.inputs.clone());
            let context = java::TargetContext {
                revision: set.revision.clone(),
                files: set.files.clone(),
                entries: conformance
                    .operations
                    .iter()
                    .map(|op| java::TargetEntry {
                        feature: op.feature.clone(),
                        method: op.binding.clone(),
                        policies: op.policies.clone(),
                    })
                    .collect(),
            };
            let discovered: Vec<PathBuf> = set.files.iter().map(PathBuf::from).collect();
            let automatic = args.targets.is_empty()
                && !args
                    .entry
                    .as_ref()
                    .is_some_and(|path| path.to_string_lossy().ends_with(".target.vitrifi"));
            let mut verified = java::verify_with_target_selection(
                root,
                &analysis,
                &args.runner,
                args.timeout.unwrap_or(120),
                Some(&context),
                automatic.then_some(discovered.as_slice()),
            )?;
            verified["conformance"] = json!(conformance);
            result = verified;
        }
    } else if args.command == "verify" {
        result["executed"] = json!(false);
    }
    if !set.current(root) {
        result["status"] = json!("stale");
        result["fresh"] = json!(false);
        result["message"] =
            json!("Target contents or discovered selection changed during this command; retry");
        if args.command == "verify" && result.get("workspace_revision").is_some() {
            evidence::save(root, "java-evidence.json", &result)?;
        }
    }
    let passed = result["status"] == "passed";
    if passed && args.command == "compile" {
        if let Some(path) = &args.out {
            let output_path = if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            };
            fs::write(
                output_path,
                serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        } else {
            evidence::save(root, "program.json", &result)?;
        }
    }
    output(args, &result);
    Ok(if passed { 0 } else { 1 })
}

fn execute_java(args: &Args, root: &Path, expect_no_targets: bool) -> Result<u8, String> {
    if args.command == "evidence" {
        let mut result = java::evidence_status_with_selection(root, &evidence_selection(args))?;
        if expect_no_targets && !targets::discover(root)?.is_empty() {
            result = stale_target_selection();
        }
        output(args, &result);
        return Ok(if result["status"] == "passed" { 0 } else { 1 });
    }
    let options = java_options(args)?;
    if args.command == "review" {
        let mut result = java::review(
            root,
            &options,
            args.base
                .as_deref()
                .ok_or("review requires --base <Git revision>")?,
        )?;
        result["workflow_changes"] = json!(workflows::review_changes(
            root,
            args.base.as_deref().unwrap_or("HEAD")
        )?);
        result["target_changes"] = json!(targets::review_changes(
            root,
            args.base.as_deref().unwrap()
        )?);
        output(args, &result);
        return Ok(if result["status"] == "passed" { 0 } else { 1 });
    }
    let analysis = java::analyze(root, &options)?;
    if expect_no_targets && !targets::discover(root)?.is_empty() {
        output(args, &stale_target_selection());
        return Ok(1);
    }
    let mut result=match args.command.as_str() {
        "generate" => java::generate(root,&analysis,args.check_generated)?,
        "check" => java::generate(root,&analysis,true)?,
        "inspect" | "compile" => {
            let value=json!({"status":java::static_status(&analysis),"analysis":analysis});
            value
        },
        "verify" => java::verify_with_target_selection(root,&analysis,&args.runner,args.timeout.unwrap_or(120),None,expect_no_targets.then_some(&[]))?,
        _ => return Err(format!("{} is not supported by the generated Java workflow; use generate, check, inspect, compile, review, verify, or evidence with --java",args.command)),
    };
    if expect_no_targets && !targets::discover(root)?.is_empty() {
        result["status"] = json!("stale");
        result["fresh"] = json!(false);
        result["message"] = stale_target_selection()["message"].clone();
        if args.command == "verify" && result.get("workspace_revision").is_some() {
            evidence::save(root, "java-evidence.json", &result)?;
        }
    }
    if args.command == "generate" && !targets::discover(root)?.is_empty() {
        result["target_conformance"] = json!("not_checked");
        result["target_hint"] = json!("Generation updates implementation facts. Run conform or check with explicit --target paths to check architecture targets.");
    }
    let passed = result["status"] == "passed";
    if passed && args.command == "compile" {
        if let Some(path) = &args.out {
            let target = if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            };
            fs::write(
                target,
                serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        } else {
            evidence::save(root, "program.json", &result)?;
        }
    }
    output(args, &result);
    Ok(if passed { 0 } else { 1 })
}

fn execute(args: &Args) -> Result<u8, String> {
    if args.command == "help" {
        print!("{HELP}");
        println!("\nGenerated Java/Spring workflow (JDK 21+):\n  generate   Derive feature-local .vitrifi files from annotated Java source\n  generate --check  Check generated snapshots without writing\n  check/inspect/review/verify --java  Analyze implementation-derived facts\n\n  --java-analyzer JAR   Or VITRIFI_JAVA_ANALYZER\n  --classpath-file FILE   Defaults to target/vitrifi-classpath.txt inside --root\n  --source-root DIR       Defaults to src/main/java inside --root\n  verify --java -- COMMAND...  Explicit instrumented test command\n  --timeout SECONDS       Verification limit; defaults to 120\n\nGenerated .vitrifi snapshots automatically select Java analysis.\n");
        println!("Executable workflow pilot:\n  workflow lint|build|check|verify [FILE.workflow.vitrifi] --root DIR\n  build validates Java actions before emitting Spring services.\n  verify accepts -- COMMAND... for instrumented scenarios.\n");
        println!("Authored architecture targets:\n  lint --target FILE      Check a .target.vitrifi without Java or implementation\n  conform --target FILE   Compare authored constraints to Java source facts\n  verify --target FILE -- COMMAND...  Check conformance and target-linked evidence\n  --target FILE           Repeat to check several feature-local targets\n\ncheck, inspect, compile, verify and evidence discover .target.vitrifi files by default.\nUse explicit --target paths in CI so deleting a target cannot remove its obligations.\n");
        return Ok(0);
    }
    if args.command == "version" {
        println!("vitrifi {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    let root = args.root.canonicalize().map_err(|e| e.to_string())?;
    if !root.is_dir() {
        return Err("--root must name a directory".into());
    }
    if args.command == "workflow" {
        return execute_workflows(args, &root);
    }
    let mut target_paths = args.targets.clone();
    if let Some(entry) = &args.entry {
        if entry.to_string_lossy().ends_with(".target.vitrifi") {
            target_paths.push(entry.clone());
        }
    }
    // Resolve only the user's root spelling, not target files or descendants.
    // This accepts root aliases (such as macOS /var) while read_inside still
    // rejects symlinks and traversal within the selected root.
    let supplied_root = if args.root.is_absolute() {
        args.root.clone()
    } else {
        env::current_dir()
            .map_err(|e| e.to_string())?
            .join(&args.root)
    };
    for path in &mut target_paths {
        if path.is_absolute() {
            if let Ok(relative) = path.strip_prefix(&supplied_root) {
                *path = relative.to_path_buf();
            }
        }
    }
    if !target_paths.is_empty() || matches!(args.command.as_str(), "lint" | "conform") {
        return execute_targets(args, &root, &target_paths);
    }
    let generated = matches!(
        args.command.as_str(),
        "check" | "inspect" | "compile" | "review" | "verify" | "source" | "evidence"
    ) && generated_entry(&root, args.entry.as_deref())?;
    let explicit_legacy = args.entry.is_some() && !args.java && !generated;
    let discover_targets = !explicit_legacy
        && matches!(
            args.command.as_str(),
            "check" | "inspect" | "compile" | "verify" | "evidence"
        );
    if discover_targets {
        let discovered = targets::discover(&root)?;
        if !discovered.is_empty() {
            return execute_targets(args, &root, &[]);
        }
    }
    if args.java
        || args.command == "generate"
        || generated
        || (!explicit_legacy
            && matches!(
                args.command.as_str(),
                "check" | "inspect" | "compile" | "verify" | "source" | "evidence" | "review"
            )
            && !workflows::discover(&root)?.is_empty())
    {
        return execute_java(args, &root, discover_targets);
    }
    if args.command == "init" {
        let entry = args
            .entry
            .as_deref()
            .unwrap_or(Path::new("feature.vitrifi"));
        if entry.extension().and_then(|e| e.to_str()) != Some("vitrifi") {
            return Err("Feature source must have a .vitrifi extension".into());
        }
        let target = if entry.is_absolute() {
            entry.to_path_buf()
        } else {
            root.join(entry)
        };
        let parent = target
            .parent()
            .unwrap()
            .canonicalize()
            .map_err(|_| "Create the feature directory first")?;
        if !parent.starts_with(&root) {
            return Err("Feature must be inside --root".into());
        }
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| format!("Cannot create {}: {e}", target.display()))?;
        file.write_all(b"// Keep this file beside the feature implementation.\nfeature Example {\n  purpose \"Describe this capability.\";\n\n  requirement Behavior \"Describe a behavior the implementation must preserve.\";\n\n  // Import other feature contracts with: use \"../other/feature.vitrifi\" as Other;\n  // (use declarations go before the feature block.)\n}\n").map_err(|e|e.to_string())?;
        output(
            args,
            &json!({"created":target,"next":"Add typed operations, bindings and relationships in this feature file."}),
        );
        return Ok(0);
    }
    if args.command == "evidence" {
        let value = evidence::status(&root)?;
        let code = if value["status"] == "passed" { 0 } else { 1 };
        output(args, &value);
        return Ok(code);
    }
    if args.command == "review" {
        let mut result = review::review(
            &root,
            args.base
                .as_deref()
                .ok_or("review requires --base <Git revision>")?,
            args.entry.as_deref(),
        )?;
        result["workflow_changes"] = json!(workflows::review_changes(
            &root,
            args.base.as_deref().unwrap_or("HEAD")
        )?);
        result["target_changes"] = json!(targets::review_changes(
            &root,
            args.base.as_deref().unwrap()
        )?);
        output(args, &result);
        return Ok(0);
    }
    if !["check", "compile", "inspect", "source", "verify"].contains(&args.command.as_str()) {
        return Err(format!("Unknown command {}\n{HELP}", args.command));
    }
    let program = compiler::compile(&root, args.entry.as_deref())?;
    let passed = program.diagnostics.is_empty();
    match args.command.as_str() {
        "check" => {
            if args.json {
                output(
                    args,
                    &json!({"status":if passed{"passed"}else{"failed"},"revision":program.revision,"features":program.modules.len(),"source_files":program.host_files.len(),"diagnostics":program.diagnostics,"limitations":program.limitations}),
                );
            } else {
                print_diagnostics(&program);
                println!(
                    "{}: {} feature modules, {} source files, {} relationships",
                    if passed { "PASS" } else { "FAIL" },
                    program.modules.len(),
                    program.host_files.len(),
                    program.edges.len()
                );
                for limitation in &program.limitations {
                    println!("  {limitation}");
                }
            }
        }
        "inspect" => {
            if args.json {
                output(args, &json!(program));
            } else {
                inspect(&program);
            }
        }
        "compile" => {
            if !passed {
                if args.json {
                    output(args, &json!(program));
                } else {
                    print_diagnostics(&program);
                }
                return Ok(1);
            }
            let value = json!(program);
            let path = if let Some(path) = &args.out {
                let target = if path.is_absolute() {
                    path.clone()
                } else {
                    root.join(path)
                };
                fs::write(&target, serde_json::to_vec_pretty(&value).unwrap())
                    .map_err(|e| e.to_string())?;
                target
            } else {
                evidence::save(&root, "program.json", &value)?;
                root.join(".vitrifi/program.json")
            };
            output(
                args,
                &json!({"compiled":path,"revision":program.revision,"features":program.modules.len()}),
            );
        }
        "source" => {
            if !passed {
                if args.json {
                    output(
                        args,
                        &json!({"status":"compile_failed","diagnostics":program.diagnostics}),
                    );
                } else {
                    print_diagnostics(&program);
                }
                return Ok(1);
            }
            let member = args
                .member
                .as_deref()
                .ok_or("source requires a feature entry and an operation name")?;
            let mut found = vec![];
            for module in &program.modules {
                for d in &module.declarations {
                    if d.name == member || format!("{}::{}", module.name, d.name) == member {
                        if let DeclBody::Operation { binding, .. } = &d.body {
                            found.push((module, binding));
                        }
                    }
                }
            }
            if found.len() != 1 {
                return Err(format!(
                    "Expected one operation {member}, found {}",
                    found.len()
                ));
            }
            let (module, binding) = found[0];
            let path = compiler::relative_path(&module.path, &binding.path)?;
            let file = program.host_files.iter().find(|f| f.path == path).unwrap();
            let symbol = file
                .symbols
                .iter()
                .find(|s| s.name == binding.symbol)
                .unwrap();
            let raw = compiler::read_inside(&root, &path)?;
            if program.inputs.get(&path) != Some(&compiler::hash_bytes(raw.as_bytes())) {
                return Err("Source changed after compilation; retry source expansion".into());
            }
            let source = raw
                .lines()
                .skip(symbol.line - 1)
                .take(symbol.end_line - symbol.line + 1)
                .collect::<Vec<_>>()
                .join("\n");
            if args.json {
                output(
                    args,
                    &json!({"path":path,"line":symbol.line,"source":source,"revision":program.revision}),
                );
            } else {
                println!("{path}:{}\n\n{source}", symbol.line);
            }
        }
        "verify" => {
            if !passed {
                if args.json {
                    output(
                        args,
                        &json!({"status":"compile_failed","diagnostics":program.diagnostics}),
                    );
                } else {
                    print_diagnostics(&program);
                }
                return Ok(1);
            }
            let value = evidence::verify(&root, &program)?;
            let code = if value["status"] == "passed" { 0 } else { 1 };
            output(args, &value);
            return Ok(code);
        }
        _ => unreachable!(),
    }
    Ok(if passed { 0 } else { 1 })
}
fn main() -> ExitCode {
    match arguments() {
        Ok(args) => match execute(&args) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                if args.json {
                    println!("{}", json!({"error":error}));
                } else {
                    eprintln!("vitrifi: {error}");
                }
                ExitCode::from(2)
            }
        },
        Err(error) => {
            eprintln!("vitrifi: {error}");
            ExitCode::from(2)
        }
    }
}
