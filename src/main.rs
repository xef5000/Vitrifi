use serde_json::{json, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use vitrifi::{compiler, evidence, model::*, review};

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
}
fn arguments() -> Result<Args, String> {
    let mut args = Args {
        root: env::current_dir().map_err(|e| e.to_string())?,
        ..Args::default()
    };
    let mut it = env::args().skip(1);
    while let Some(value) = it.next() {
        match value.as_str() {
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
fn execute(args: &Args) -> Result<u8, String> {
    if args.command == "help" {
        print!("{HELP}");
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
        let result = review::review(
            &root,
            args.base
                .as_deref()
                .ok_or("review requires --base <Git revision>")?,
            args.entry.as_deref(),
        )?;
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
