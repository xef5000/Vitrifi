//! Compiler acceptance tests use actual feature files and real host parsers.
//! None of these tests evaluates host implementations or verification commands.
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use vitrifi::{compiler::compile, model::Program};

#[test]
fn public_contracts_cannot_leak_private_types_or_effect_resources() {
    let fixture = Fixture::new();
    fixture.write(
        "feature.vitrifi",
        r#"feature Example {
      type Internal;
      resource State;
      export operation load() -> Option<Internal> {
        bind python "./app.py" symbol "load";
        effects [read State];
      }
    }"#,
    );
    fixture.write("app.py", "def load(): return None\n");
    let program = fixture.compile("feature.vitrifi");
    assert_eq!(
        program
            .diagnostics
            .iter()
            .filter(|d| d.code == "PRIVATE_INTERFACE")
            .count(),
        2
    );
}

const STORE: &str = r#"
feature Store.Carts {
  export type Cart;
  export resource Carts;
  export requirement Saved "The cart is saved.";
  export boundary Storage ["./*.py"] allows [];
  export operation save(cart: Cart) -> Unit {
    bind python "./repo.py" symbol "save";
    effects [write Carts];
    ensures [Saved];
  }
}
"#;

const CHECKOUT: &str = r#"
use "../store/cart.vitrifi" as Store;
feature Checkout.Discounts {
  purpose "Apply a discount through the cart capability.";
  boundary Domain ["./*.py"] allows [Store.Storage];
  export operation apply(cart: Store.Cart) -> Unit {
    bind python "./app.py" symbol "apply";
    effects [write Store.Carts];
    ensures [Store.Saved];
    calls [Store.save];
  }
}
"#;

struct Fixture {
    directory: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn checkout() -> Self {
        let fixture = Self::new();
        fixture.write("domain/discounts.vitrifi", CHECKOUT);
        fixture.write("store/cart.vitrifi", STORE);
        fixture.write(
            "domain/app.py",
            "from store.repo import save\n\ndef apply(cart):\n    save(cart)\n",
        );
        fixture.write("store/repo.py", "def save(cart):\n    pass\n");
        fixture
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root().join(relative)
    }

    fn write(&self, relative: &str, source: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn compile(&self, entry: &str) -> Program {
        compile(self.root(), Some(Path::new(entry))).unwrap()
    }

    fn checkout_program(&self) -> Program {
        self.compile("domain/discounts.vitrifi")
    }
}

fn has_code(program: &Program, code: &str) {
    assert!(
        program.diagnostics.iter().any(|d| d.code == code),
        "Expected {code}; diagnostics: {:#?}",
        program.diagnostics
    );
}

fn clean(program: &Program) {
    assert!(program.diagnostics.is_empty(), "{:#?}", program.diagnostics);
}

#[test]
fn imports_link_exported_types_resources_requirements_and_operations() {
    let fixture = Fixture::checkout();
    let program = fixture.checkout_program();
    clean(&program);
    assert_eq!(program.modules.len(), 2);
    assert_eq!(program.host_files.len(), 2);
    for (relation, to) in [
        ("calls", "Store.Carts::save"),
        ("write", "Store.Carts::Carts"),
        ("ensures", "Store.Carts::Saved"),
        ("binds", "domain/app.py::apply"),
    ] {
        assert!(
            program
                .edges
                .iter()
                .any(|e| e.from == "Checkout.Discounts::apply"
                    && e.relation == relation
                    && e.to == to),
            "Missing {relation} edge to {to}"
        );
    }
    assert!(program
        .edges
        .iter()
        .any(|e| e.relation == "imports" && e.from == "domain/app.py" && e.to == "store/repo.py"));
}

#[test]
fn relationship_targets_have_semantic_kinds() {
    for invalid in [
        CHECKOUT.replace("ensures [Store.Saved]", "ensures [Store.Carts]"),
        CHECKOUT.replace("calls [Store.save]", "calls [Store.Saved]"),
        CHECKOUT.replace("write Store.Carts", "write Store.Cart"),
        CHECKOUT.replace("allows [Store.Storage]", "allows [Store.save]"),
    ] {
        let fixture = Fixture::checkout();
        fixture.write("domain/discounts.vitrifi", &invalid);
        has_code(&fixture.checkout_program(), "TYPE_MISMATCH");
    }
}

#[test]
fn imports_cannot_reach_private_feature_members() {
    let fixture = Fixture::checkout();
    fixture.write(
        "store/cart.vitrifi",
        &STORE.replace("export operation save", "operation save"),
    );
    let program = fixture.checkout_program();
    has_code(&program, "PRIVATE_MEMBER");
    assert!(!program
        .edges
        .iter()
        .any(|e| e.relation == "calls" && e.to == "Store.Carts::save"));
}

#[test]
fn unknown_types_and_wrong_generic_arity_fail_compilation() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("cart: Store.Cart", "cart: Missing"),
    );
    has_code(&fixture.checkout_program(), "UNKNOWN_NAME");
    for ty in ["List", "Result<String>", "String<Int>", "Store.Cart<Int>"] {
        fixture.write(
            "domain/discounts.vitrifi",
            &CHECKOUT.replace("cart: Store.Cart", &format!("cart: {ty}")),
        );
        has_code(&fixture.checkout_program(), "TYPE_ARITY");
    }
}

#[test]
fn valid_nested_generic_types_are_checked_recursively() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("cart: Store.Cart", "cart: List<Option<Store.Cart>>"),
    );
    clean(&fixture.checkout_program());
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("cart: Store.Cart", "cart: List<Option<Missing>>"),
    );
    has_code(&fixture.checkout_program(), "UNKNOWN_NAME");
}

#[test]
fn one_host_symbol_has_one_owner_across_features() {
    let fixture = Fixture::checkout();
    let additional = r#"
  operation duplicate() -> Unit {
    bind python "../store/repo.py" symbol "save";
  }
"#;
    let insertion = CHECKOUT.rfind('}').unwrap();
    let changed = format!(
        "{}{}{}",
        &CHECKOUT[..insertion],
        additional,
        &CHECKOUT[insertion..]
    );
    fixture.write("domain/discounts.vitrifi", &changed);
    has_code(&fixture.checkout_program(), "DUPLICATE_BINDING");
}

#[test]
fn declared_call_effects_must_be_included_in_callers_contract() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("effects [write Store.Carts]", "effects [read Store.Carts]"),
    );
    let program = fixture.checkout_program();
    has_code(&program, "UNDECLARED_EFFECT");
    let diagnostic = program
        .diagnostics
        .iter()
        .find(|d| d.code == "UNDECLARED_EFFECT")
        .unwrap();
    assert!(diagnostic.message.contains("write Store.Carts::Carts"));
    assert_eq!(diagnostic.location.path, "domain/discounts.vitrifi");
}

#[test]
fn effect_identity_uses_resolved_resources_not_import_alias_spelling() {
    let fixture = Fixture::checkout();
    let renamed = CHECKOUT
        .replace("as Store;", "as CartRepository;")
        .replace("Store.", "CartRepository.");
    fixture.write("domain/discounts.vitrifi", &renamed);
    clean(&fixture.checkout_program());
}

#[test]
fn missing_host_symbol_and_wrong_binding_language_are_errors() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("symbol \"apply\"", "symbol \"missing\""),
    );
    has_code(&fixture.checkout_program(), "MISSING_HOST_SYMBOL");
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("bind python", "bind javascript"),
    );
    has_code(&fixture.checkout_program(), "BINDING_LANGUAGE");
}

#[test]
fn host_imports_are_checked_against_declared_boundaries() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("allows [Store.Storage]", "allows []"),
    );
    let program = fixture.checkout_program();
    has_code(&program, "BOUNDARY_VIOLATION");
    let diagnostic = program
        .diagnostics
        .iter()
        .find(|d| d.code == "BOUNDARY_VIOLATION")
        .unwrap();
    assert_eq!(diagnostic.location.path, "domain/app.py");
    assert_eq!(diagnostic.location.line, 1);
    assert!(diagnostic.message.contains("domain/discounts.vitrifi:"));
}

#[test]
fn module_relative_imports_bindings_and_boundary_paths_are_normalized() {
    let fixture = Fixture::new();
    fixture.write(
        "packages/cart/model/cart.vitrifi",
        r#"
feature Cart {
  export type View;
  export operation load() -> View { bind python "../implementation/cart.py" symbol "load"; }
}
"#,
    );
    fixture.write(
        "packages/cart/implementation/cart.py",
        "def load():\n    return {}\n",
    );
    fixture.write(
        "features/checkout/checkout.vitrifi",
        r#"
use "../../packages/cart/model/cart.vitrifi" as Cart;
feature Checkout {
  boundary App ["./implementation/*.py"] allows [];
  operation apply() -> Cart.View {
    bind python "./implementation/./checkout.py" symbol "apply";
    calls [Cart.load];
  }
}
"#,
    );
    fixture.write(
        "features/checkout/implementation/checkout.py",
        "def apply():\n    return {}\n",
    );
    let program = fixture.compile("features/checkout/checkout.vitrifi");
    clean(&program);
    assert!(program
        .host_files
        .iter()
        .any(|f| f.path == "packages/cart/implementation/cart.py"));
    assert!(program
        .host_files
        .iter()
        .any(|f| f.path == "features/checkout/implementation/checkout.py"));
}

#[test]
fn entry_compilation_ignores_unrelated_invalid_features_and_host_files() {
    let fixture = Fixture::checkout();
    fixture.write("unrelated/broken.vitrifi", "feature Broken { ??? }");
    fixture.write("unrelated/broken.py", "def ( invalid syntax\n");
    let program = fixture.checkout_program();
    clean(&program);
    assert_eq!(program.modules.len(), 2);
    let all = compile(fixture.root(), None).unwrap();
    assert!(all
        .diagnostics
        .iter()
        .any(|d| d.location.path == "unrelated/broken.vitrifi"));
}

#[test]
fn compilation_revision_tracks_the_import_closure_only() {
    let fixture = Fixture::checkout();
    fixture.write("unrelated/readme.md", "Initial unrelated content\n");
    let initial = fixture.checkout_program();
    clean(&initial);
    fixture.write("unrelated/readme.md", "Changed unrelated content\n");
    assert_eq!(initial.revision, fixture.checkout_program().revision);
    fixture.write(
        "store/cart.vitrifi",
        &STORE.replace("The cart is saved.", "The cart is saved atomically."),
    );
    let changed = fixture.checkout_program();
    clean(&changed);
    assert_ne!(initial.revision, changed.revision);
}

#[test]
fn paths_cannot_escape_the_repository() {
    let fixture = Fixture::checkout();
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("../store/cart.vitrifi", "../../outside.vitrifi"),
    );
    has_code(&fixture.checkout_program(), "IMPORT_PATH");
    fixture.write(
        "domain/discounts.vitrifi",
        &CHECKOUT.replace("./app.py", "../../outside.py"),
    );
    has_code(&fixture.checkout_program(), "BINDING_PATH");
}

#[test]
fn cyclic_module_imports_are_linked_once() {
    let fixture = Fixture::new();
    fixture.write(
        "a.vitrifi",
        r#"
use "./b.vitrifi" as B;
feature A {
  export requirement Ready "Ready to run.";
  operation first() -> Unit {
    bind python "./a.py" symbol "first";
    requires [B.Ready];
  }
}
"#,
    );
    fixture.write(
        "b.vitrifi",
        r#"
use "./a.vitrifi" as A;
feature B {
  export requirement Ready "Ready to run.";
  operation second() -> Unit {
    bind python "./b.py" symbol "second";
    requires [A.Ready];
  }
}
"#,
    );
    fixture.write("a.py", "def first():\n    pass\n");
    fixture.write("b.py", "def second():\n    pass\n");
    let program = fixture.compile("a.vitrifi");
    clean(&program);
    assert_eq!(program.modules.len(), 2);
}

#[test]
fn empty_commands_and_invalid_check_directories_are_compile_errors() {
    let fixture = Fixture::new();
    fixture.write(
        "checks.vitrifi",
        r#"
feature Checks {
  requirement Ready "It works.";
  check Behavior { run []; covers [Ready]; cwd "./missing"; timeout 0; }
}
"#,
    );
    let program = fixture.compile("checks.vitrifi");
    for code in ["EMPTY_COMMAND", "CHECK_DIRECTORY", "CHECK_TIMEOUT"] {
        has_code(&program, code);
    }
}
