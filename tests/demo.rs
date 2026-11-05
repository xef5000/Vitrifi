use std::{collections::BTreeSet, path::Path};
use vitrifi::{compiler, model::DeclBody};

#[test]
fn real_example_describes_the_complete_product_feature_in_one_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/checkout");
    let program = compiler::compile(&root, None).unwrap();
    assert!(program.diagnostics.is_empty(), "{:?}", program.diagnostics);
    assert_eq!(program.modules.len(), 3);
    let languages: BTreeSet<_> = program
        .host_files
        .iter()
        .map(|f| f.language.as_str())
        .collect();
    assert_eq!(
        languages,
        BTreeSet::from(["python", "ruby", "javascript", "typescript", "java"])
    );
    let feature = program
        .modules
        .iter()
        .find(|m| m.name == "Checkout.Discounts")
        .unwrap();
    assert_eq!(feature.path, "discounts.vitrifi");
    assert_eq!(
        feature
            .declarations
            .iter()
            .filter(|d| matches!(d.body, DeclBody::Operation { .. }))
            .count(),
        4
    );
    let focused = compiler::compile(&root, Some(Path::new("discounts.vitrifi"))).unwrap();
    assert_eq!(focused.modules.len(), 2); // Product feature plus shared storage, without audit.
    assert!(focused.diagnostics.is_empty(), "{:?}", focused.diagnostics);
}
