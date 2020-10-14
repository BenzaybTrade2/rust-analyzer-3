use std::collections::BTreeSet;

use syntax::{ast, AstNode, TextRange};

use crate::{
    assist_context::{AssistContext, Assists},
    utils::import_assets::{ImportAssets, ImportCandidate},
    utils::mod_path_to_ast,
    AssistId, AssistKind, GroupLabel,
};

// Assist: qualify_path
//
// If the name is unresolved, provides all possible qualified paths for it.
//
// ```
// fn main() {
//     let map = HashMap<|>::new();
// }
// # pub mod std { pub mod collections { pub struct HashMap { } } }
// ```
// ->
// ```
// fn main() {
//     let map = std::collections::HashMap::new();
// }
// # pub mod std { pub mod collections { pub struct HashMap { } } }
// ```
pub(crate) fn qualify_path(acc: &mut Assists, ctx: &AssistContext) -> Option<()> {
    let import_assets =
        if let Some(path_under_caret) = ctx.find_node_at_offset_with_descend::<ast::Path>() {
            ImportAssets::for_regular_path(path_under_caret, &ctx.sema)
        } else if let Some(method_under_caret) =
            ctx.find_node_at_offset_with_descend::<ast::MethodCallExpr>()
        {
            ImportAssets::for_method_call(method_under_caret, &ctx.sema)
        } else {
            None
        }?;
    let proposed_imports = import_assets.search_for_relative_paths(&ctx.sema);
    if proposed_imports.is_empty() {
        return None;
    }

    let range = ctx.sema.original_range(import_assets.syntax_under_caret()).range;
    match import_assets.import_candidate() {
        ImportCandidate::QualifierStart(candidate) => {
            let path = ast::Path::cast(import_assets.syntax_under_caret().clone())?;
            let segment = path.segment()?;
            qualify_path_qualifier_start(acc, proposed_imports, range, segment, &candidate.name)
        }
        ImportCandidate::UnqualifiedName(candidate) => {
            qualify_path_unqualified_name(acc, proposed_imports, range, &candidate.name)
        }
        ImportCandidate::TraitAssocItem(candidate) => {
            let path = ast::Path::cast(import_assets.syntax_under_caret().clone())?;
            let (qualifier, segment) = (path.qualifier()?, path.segment()?);
            qualify_path_trait_assoc_item(
                acc,
                proposed_imports,
                range,
                qualifier,
                segment,
                &candidate.name,
            )
        }
        ImportCandidate::TraitMethod(candidate) => {
            let mcall_expr = ast::MethodCallExpr::cast(import_assets.syntax_under_caret().clone())?;
            let receiver = mcall_expr.receiver()?;
            let name_ref = mcall_expr.name_ref()?;
            qualify_path_trait_method(
                acc,
                proposed_imports,
                range,
                receiver,
                name_ref,
                &candidate.name,
            )
        }
    };
    Some(())
}

// a test that covers this -> `associated_struct_const`
fn qualify_path_qualifier_start(
    acc: &mut Assists,
    proposed_imports: BTreeSet<hir::ModPath>,
    range: TextRange,
    segment: ast::PathSegment,
    qualifier_start: &str,
) {
    let group_label = GroupLabel(format!("Qualify {}", qualifier_start));
    for import in proposed_imports {
        acc.add_group(
            &group_label,
            AssistId("qualify_path", AssistKind::QuickFix),
            format!("Qualify with `{}`", &import),
            range,
            |builder| {
                let import = mod_path_to_ast(&import);
                builder.replace(range, format!("{}::{}", import, segment));
            },
        );
    }
}

// a test that covers this -> `applicable_when_found_an_import_partial`
fn qualify_path_unqualified_name(
    acc: &mut Assists,
    proposed_imports: BTreeSet<hir::ModPath>,
    range: TextRange,
    name: &str,
) {
    let group_label = GroupLabel(format!("Qualify {}", name));
    for import in proposed_imports {
        acc.add_group(
            &group_label,
            AssistId("qualify_path", AssistKind::QuickFix),
            format!("Qualify as `{}`", &import),
            range,
            |builder| builder.replace(range, mod_path_to_ast(&import).to_string()),
        );
    }
}

// a test that covers this -> `associated_trait_const`
fn qualify_path_trait_assoc_item(
    acc: &mut Assists,
    proposed_imports: BTreeSet<hir::ModPath>,
    range: TextRange,
    qualifier: ast::Path,
    segment: ast::PathSegment,
    trait_assoc_item_name: &str,
) {
    let group_label = GroupLabel(format!("Qualify {}", trait_assoc_item_name));
    for import in proposed_imports {
        acc.add_group(
            &group_label,
            AssistId("qualify_path", AssistKind::QuickFix),
            format!("Qualify with cast as `{}`", &import),
            range,
            |builder| {
                let import = mod_path_to_ast(&import);
                builder.replace(range, format!("<{} as {}>::{}", qualifier, import, segment));
            },
        );
    }
}

// a test that covers this -> `trait_method`
fn qualify_path_trait_method(
    acc: &mut Assists,
    proposed_imports: BTreeSet<hir::ModPath>,
    range: TextRange,
    receiver: ast::Expr,
    name_ref: ast::NameRef,
    trait_method_name: &str,
) {
    let group_label = GroupLabel(format!("Qualify {}", trait_method_name));
    for import in proposed_imports {
        acc.add_group(
            &group_label,
            AssistId("qualify_path", AssistKind::QuickFix), // < Does this still count as quickfix?
            format!("Qualify `{}`", &import),
            range,
            |builder| {
                let import = mod_path_to_ast(&import);
                // TODO: check the receiver self type and emit refs accordingly, don't discard other function parameters
                builder.replace(range, format!("{}::{}(&{})", import, name_ref, receiver));
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{check_assist, check_assist_not_applicable, check_assist_target};
    #[test]
    fn applicable_when_found_an_import_partial() {
        check_assist(
            qualify_path,
            r"
            mod std {
                pub mod fmt {
                    pub struct Formatter;
                }
            }

            use std::fmt;

            <|>Formatter
            ",
            r"
            mod std {
                pub mod fmt {
                    pub struct Formatter;
                }
            }

            use std::fmt;

            fmt::Formatter
            ",
        );
    }

    #[test]
    fn applicable_when_found_an_import() {
        check_assist(
            qualify_path,
            r"
            <|>PubStruct

            pub mod PubMod {
                pub struct PubStruct;
            }
            ",
            r"
            PubMod::PubStruct

            pub mod PubMod {
                pub struct PubStruct;
            }
            ",
        );
    }

    #[test]
    fn applicable_in_macros() {
        check_assist(
            qualify_path,
            r"
            macro_rules! foo {
                ($i:ident) => { fn foo(a: $i) {} }
            }
            foo!(Pub<|>Struct);

            pub mod PubMod {
                pub struct PubStruct;
            }
            ",
            r"
            macro_rules! foo {
                ($i:ident) => { fn foo(a: $i) {} }
            }
            foo!(PubMod::PubStruct);

            pub mod PubMod {
                pub struct PubStruct;
            }
            ",
        );
    }

    #[test]
    fn applicable_when_found_multiple_imports() {
        check_assist(
            qualify_path,
            r"
            PubSt<|>ruct

            pub mod PubMod1 {
                pub struct PubStruct;
            }
            pub mod PubMod2 {
                pub struct PubStruct;
            }
            pub mod PubMod3 {
                pub struct PubStruct;
            }
            ",
            r"
            PubMod3::PubStruct

            pub mod PubMod1 {
                pub struct PubStruct;
            }
            pub mod PubMod2 {
                pub struct PubStruct;
            }
            pub mod PubMod3 {
                pub struct PubStruct;
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_already_imported_types() {
        check_assist_not_applicable(
            qualify_path,
            r"
            use PubMod::PubStruct;

            PubStruct<|>

            pub mod PubMod {
                pub struct PubStruct;
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_types_with_private_paths() {
        check_assist_not_applicable(
            qualify_path,
            r"
            PrivateStruct<|>

            pub mod PubMod {
                struct PrivateStruct;
            }
            ",
        );
    }

    #[test]
    fn not_applicable_when_no_imports_found() {
        check_assist_not_applicable(
            qualify_path,
            "
            PubStruct<|>",
        );
    }

    #[test]
    fn not_applicable_in_import_statements() {
        check_assist_not_applicable(
            qualify_path,
            r"
            use PubStruct<|>;

            pub mod PubMod {
                pub struct PubStruct;
            }",
        );
    }

    #[test]
    fn qualify_function() {
        check_assist(
            qualify_path,
            r"
            test_function<|>

            pub mod PubMod {
                pub fn test_function() {};
            }
            ",
            r"
            PubMod::test_function

            pub mod PubMod {
                pub fn test_function() {};
            }
            ",
        );
    }

    #[test]
    fn qualify_macro() {
        check_assist(
            qualify_path,
            r"
//- /lib.rs crate:crate_with_macro
#[macro_export]
macro_rules! foo {
    () => ()
}

//- /main.rs crate:main deps:crate_with_macro
fn main() {
    foo<|>
}
",
            r"
fn main() {
    crate_with_macro::foo
}
",
        );
    }

    #[test]
    fn qualify_path_target() {
        check_assist_target(
            qualify_path,
            r"
            struct AssistInfo {
                group_label: Option<<|>GroupLabel>,
            }

            mod m { pub struct GroupLabel; }
            ",
            "GroupLabel",
        )
    }

    #[test]
    fn not_applicable_when_path_start_is_imported() {
        check_assist_not_applicable(
            qualify_path,
            r"
            pub mod mod1 {
                pub mod mod2 {
                    pub mod mod3 {
                        pub struct TestStruct;
                    }
                }
            }

            use mod1::mod2;
            fn main() {
                mod2::mod3::TestStruct<|>
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_imported_function() {
        check_assist_not_applicable(
            qualify_path,
            r"
            pub mod test_mod {
                pub fn test_function() {}
            }

            use test_mod::test_function;
            fn main() {
                test_function<|>
            }
            ",
        );
    }

    #[test]
    fn associated_struct_function() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub struct TestStruct {}
                impl TestStruct {
                    pub fn test_function() {}
                }
            }

            fn main() {
                TestStruct::test_function<|>
            }
            ",
            r"
            mod test_mod {
                pub struct TestStruct {}
                impl TestStruct {
                    pub fn test_function() {}
                }
            }

            fn main() {
                test_mod::TestStruct::test_function
            }
            ",
        );
    }

    #[test]
    fn associated_struct_const() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub struct TestStruct {}
                impl TestStruct {
                    const TEST_CONST: u8 = 42;
                }
            }

            fn main() {
                TestStruct::TEST_CONST<|>
            }
            ",
            r"
            mod test_mod {
                pub struct TestStruct {}
                impl TestStruct {
                    const TEST_CONST: u8 = 42;
                }
            }

            fn main() {
                test_mod::TestStruct::TEST_CONST
            }
            ",
        );
    }

    #[test]
    fn associated_trait_function() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_function();
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_function() {}
                }
            }

            fn main() {
                test_mod::TestStruct::test_function<|>
            }
            ",
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_function();
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_function() {}
                }
            }

            fn main() {
                <test_mod::TestStruct as test_mod::TestTrait>::test_function
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_imported_trait_for_function() {
        check_assist_not_applicable(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_function();
                }
                pub trait TestTrait2 {
                    fn test_function();
                }
                pub enum TestEnum {
                    One,
                    Two,
                }
                impl TestTrait2 for TestEnum {
                    fn test_function() {}
                }
                impl TestTrait for TestEnum {
                    fn test_function() {}
                }
            }

            use test_mod::TestTrait2;
            fn main() {
                test_mod::TestEnum::test_function<|>;
            }
            ",
        )
    }

    #[test]
    fn associated_trait_const() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    const TEST_CONST: u8;
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    const TEST_CONST: u8 = 42;
                }
            }

            fn main() {
                test_mod::TestStruct::TEST_CONST<|>
            }
            ",
            r"
            mod test_mod {
                pub trait TestTrait {
                    const TEST_CONST: u8;
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    const TEST_CONST: u8 = 42;
                }
            }

            fn main() {
                <test_mod::TestStruct as test_mod::TestTrait>::TEST_CONST
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_imported_trait_for_const() {
        check_assist_not_applicable(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    const TEST_CONST: u8;
                }
                pub trait TestTrait2 {
                    const TEST_CONST: f64;
                }
                pub enum TestEnum {
                    One,
                    Two,
                }
                impl TestTrait2 for TestEnum {
                    const TEST_CONST: f64 = 42.0;
                }
                impl TestTrait for TestEnum {
                    const TEST_CONST: u8 = 42;
                }
            }

            use test_mod::TestTrait2;
            fn main() {
                test_mod::TestEnum::TEST_CONST<|>;
            }
            ",
        )
    }

    #[test]
    fn trait_method() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_struct.test_meth<|>od()
            }
            ",
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_mod::TestTrait::test_method(&test_struct)
            }
            ",
        );
    }

    #[test]
    fn trait_method_multi_params() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self, test: i32);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self, test: i32) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_struct.test_meth<|>od(42)
            }
            ",
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self, test: i32);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self, test: i32) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_mod::TestTrait::test_method(&test_struct, 42)
            }
            ",
        );
    }

    #[test]
    fn trait_method_consume() {
        check_assist(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(self) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_struct.test_meth<|>od()
            }
            ",
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(self) {}
                }
            }

            fn main() {
                let test_struct = test_mod::TestStruct {};
                test_mod::TestTrait::test_method(test_struct)
            }
            ",
        );
    }

    #[test]
    fn trait_method_cross_crate() {
        check_assist(
            qualify_path,
            r"
            //- /main.rs crate:main deps:dep
            fn main() {
                let test_struct = dep::test_mod::TestStruct {};
                test_struct.test_meth<|>od()
            }
            //- /dep.rs crate:dep
            pub mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self) {}
                }
            }
            ",
            r"
            fn main() {
                let test_struct = dep::test_mod::TestStruct {};
                dep::test_mod::TestTrait::test_method(&test_struct)
            }
            ",
        );
    }

    #[test]
    fn assoc_fn_cross_crate() {
        check_assist(
            qualify_path,
            r"
            //- /main.rs crate:main deps:dep
            fn main() {
                dep::test_mod::TestStruct::test_func<|>tion
            }
            //- /dep.rs crate:dep
            pub mod test_mod {
                pub trait TestTrait {
                    fn test_function();
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_function() {}
                }
            }
            ",
            r"
            fn main() {
                <dep::test_mod::TestStruct as dep::test_mod::TestTrait>::test_function
            }
            ",
        );
    }

    #[test]
    fn assoc_const_cross_crate() {
        check_assist(
            qualify_path,
            r"
            //- /main.rs crate:main deps:dep
            fn main() {
                dep::test_mod::TestStruct::CONST<|>
            }
            //- /dep.rs crate:dep
            pub mod test_mod {
                pub trait TestTrait {
                    const CONST: bool;
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    const CONST: bool = true;
                }
            }
            ",
            r"
            fn main() {
                <dep::test_mod::TestStruct as dep::test_mod::TestTrait>::CONST
            }
            ",
        );
    }

    #[test]
    fn assoc_fn_as_method_cross_crate() {
        check_assist_not_applicable(
            qualify_path,
            r"
            //- /main.rs crate:main deps:dep
            fn main() {
                let test_struct = dep::test_mod::TestStruct {};
                test_struct.test_func<|>tion()
            }
            //- /dep.rs crate:dep
            pub mod test_mod {
                pub trait TestTrait {
                    fn test_function();
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_function() {}
                }
            }
            ",
        );
    }

    #[test]
    fn private_trait_cross_crate() {
        check_assist_not_applicable(
            qualify_path,
            r"
            //- /main.rs crate:main deps:dep
            fn main() {
                let test_struct = dep::test_mod::TestStruct {};
                test_struct.test_meth<|>od()
            }
            //- /dep.rs crate:dep
            pub mod test_mod {
                trait TestTrait {
                    fn test_method(&self);
                }
                pub struct TestStruct {}
                impl TestTrait for TestStruct {
                    fn test_method(&self) {}
                }
            }
            ",
        );
    }

    #[test]
    fn not_applicable_for_imported_trait_for_method() {
        check_assist_not_applicable(
            qualify_path,
            r"
            mod test_mod {
                pub trait TestTrait {
                    fn test_method(&self);
                }
                pub trait TestTrait2 {
                    fn test_method(&self);
                }
                pub enum TestEnum {
                    One,
                    Two,
                }
                impl TestTrait2 for TestEnum {
                    fn test_method(&self) {}
                }
                impl TestTrait for TestEnum {
                    fn test_method(&self) {}
                }
            }

            use test_mod::TestTrait2;
            fn main() {
                let one = test_mod::TestEnum::One;
                one.test<|>_method();
            }
            ",
        )
    }

    #[test]
    fn dep_import() {
        check_assist(
            qualify_path,
            r"
//- /lib.rs crate:dep
pub struct Struct;

//- /main.rs crate:main deps:dep
fn main() {
    Struct<|>
}
",
            r"
fn main() {
    dep::Struct
}
",
        );
    }

    #[test]
    fn whole_segment() {
        // Tests that only imports whose last segment matches the identifier get suggested.
        check_assist(
            qualify_path,
            r"
//- /lib.rs crate:dep
pub mod fmt {
    pub trait Display {}
}

pub fn panic_fmt() {}

//- /main.rs crate:main deps:dep
struct S;

impl f<|>mt::Display for S {}
",
            r"
struct S;

impl dep::fmt::Display for S {}
",
        );
    }

    #[test]
    fn macro_generated() {
        // Tests that macro-generated items are suggested from external crates.
        check_assist(
            qualify_path,
            r"
//- /lib.rs crate:dep
macro_rules! mac {
    () => {
        pub struct Cheese;
    };
}

mac!();

//- /main.rs crate:main deps:dep
fn main() {
    Cheese<|>;
}
",
            r"
fn main() {
    dep::Cheese;
}
",
        );
    }

    #[test]
    fn casing() {
        // Tests that differently cased names don't interfere and we only suggest the matching one.
        check_assist(
            qualify_path,
            r"
//- /lib.rs crate:dep
pub struct FMT;
pub struct fmt;

//- /main.rs crate:main deps:dep
fn main() {
    FMT<|>;
}
",
            r"
fn main() {
    dep::FMT;
}
",
        );
    }
}
