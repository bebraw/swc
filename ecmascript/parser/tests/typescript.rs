#![feature(test)]

extern crate test;

use crate::common::Normalizer;
use pretty_assertions::assert_eq;
use std::{
    env,
    fs::File,
    io::{self, Read},
    path::Path,
};
use swc_ecma_ast::*;
use swc_ecma_parser::{lexer::Lexer, JscTarget, PResult, Parser, StringInput, Syntax, TsConfig};
use swc_ecma_visit::FoldWith;
use test::{
    test_main, DynTestFn, Options, ShouldPanic::No, TestDesc, TestDescAndFn, TestName, TestType,
};
use testing::StdErr;
use walkdir::WalkDir;

#[path = "common/mod.rs"]
mod common;

fn add_test<F: FnOnce() + Send + 'static>(
    tests: &mut Vec<TestDescAndFn>,
    name: String,
    ignore: bool,
    f: F,
) {
    if ignore {
        return;
    }
    tests.push(TestDescAndFn {
        desc: TestDesc {
            test_type: TestType::UnitTest,
            name: TestName::DynTestName(name),
            ignore,
            should_panic: No,
            allow_fail: false,
        },
        testfn: DynTestFn(Box::new(f)),
    });
}

fn reference_tests(tests: &mut Vec<TestDescAndFn>, errors: bool) -> Result<(), io::Error> {
    let root = {
        let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
        root.push("tests");
        root.push(if errors {
            "typescript-errors"
        } else {
            "typescript"
        });
        root
    };

    eprintln!("Loading tests from {}", root.display());

    let dir = root;

    for entry in WalkDir::new(&dir).into_iter() {
        let entry = entry?;
        if entry.file_type().is_dir()
            || !(entry.file_name().to_string_lossy().ends_with(".ts")
                || entry.file_name().to_string_lossy().ends_with(".tsx"))
        {
            continue;
        }
        let file_name = entry
            .path()
            .strip_prefix(&dir)
            .expect("failed to strip prefix")
            .to_str()
            .unwrap()
            .to_string();

        let input = {
            let mut buf = String::new();
            File::open(entry.path())?.read_to_string(&mut buf)?;
            buf
        };

        // Ignore some useless tests
        let ignore = file_name.contains("tsc/es6/functionDeclarations/FunctionDeclaration7_es6")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings11_ES5")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings10_ES5")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings11_ES6")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInStrings10_ES6")
            || file_name.contains(
                "tsc/types/objectTypeLiteral/propertySignatures/propertyNamesOfReservedWords",
            )
            || file_name.contains("unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates10_ES5")
            || file_name.contains("unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates10_ES6")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates11_ES5")
            || file_name
                .contains("tsc/es6/unicodeExtendedEscapes/unicodeExtendedEscapesInTemplates11_ES6")
            || file_name.contains("tsc/es6/variableDeclarations/VariableDeclaration6_es6")
            || file_name.contains(
                "tsc/parser/ecmascriptnext/numericSeparators/parser.numericSeparators.decimal",
            );

        // Useful only for error reporting
        let ignore = ignore
            || file_name.contains(
                "tsc/types/objectTypeLiteral/callSignatures/\
                 callSignaturesWithParameterInitializers",
            )
            || file_name.contains("tsc/jsdoc/jsdocDisallowedInTypescript")
            || file_name.contains("tsc/expressions/superCalls/errorSuperCalls")
            || file_name.contains("tsc/types/rest/restElementMustBeLast");

        // Postponed
        let ignore = ignore
            || file_name.contains("tsc/jsx/inline/inlineJsxFactoryDeclarationsx")
            || file_name.contains("tsc/externalModules/typeOnly/importDefaultNamedType")
            || file_name.contains("tsc/jsx/tsxAttributeResolution5x")
            || file_name.contains("tsc/jsx/tsxErrorRecovery2x")
            || file_name.contains("tsc/jsx/tsxErrorRecovery3x")
            || file_name.contains("tsc/jsx/tsxErrorRecovery5x")
            || file_name.contains("tsc/jsx/tsxReactEmitEntitiesx/input.tsx")
            || file_name.contains("tsc/jsx/tsxTypeArgumentsJsxPreserveOutputx/input.tsx")
            || file_name.contains(
                "tsc/es7/exponentiationOperator/\
                 emitCompoundExponentiationAssignmentWithIndexingOnLHS3",
            )
            || file_name.contains("tsc/expressions/objectLiterals/objectLiteralGettersAndSetters")
            || file_name.contains("tsc/types/rest/restElementMustBeLast")
            || file_name.contains("tsc/jsx/unicodeEscapesInJsxtagsx/input.tsx")
            || file_name.contains("tsc/es6/functionDeclarations/FunctionDeclaration6_es6");

        let dir = dir.clone();
        let name = format!(
            "typescript::{}::{}",
            if errors { "error" } else { "reference" },
            file_name
        );
        add_test(tests, name, ignore, move || {
            eprintln!(
                "\n\n========== Running reference test {}\nSource:\n{}\n",
                file_name, input
            );

            let path = dir.join(&file_name);
            if errors {
                let module = with_parser(false, &path, !errors, |p| p.parse_typescript_module());

                let err = module.expect_err("should fail, but parsed as");
                if err
                    .compare_to_file(format!("{}.stderr", path.display()))
                    .is_err()
                {
                    panic!()
                }
            } else {
                with_parser(is_backtrace_enabled(), &path, !errors, |p| {
                    let module = p.parse_typescript_module()?.fold_with(&mut Normalizer {
                        drop_span: false,
                        is_test262: false,
                    });

                    let json = serde_json::to_string_pretty(&module)
                        .expect("failed to serialize module as json");

                    if StdErr::from(json.clone())
                        .compare_to_file(format!("{}.json", path.display()))
                        .is_err()
                    {
                        panic!()
                    }

                    let module = module.fold_with(&mut Normalizer {
                        drop_span: true,
                        is_test262: false,
                    });

                    let deser = match serde_json::from_str::<Module>(&json) {
                        Ok(v) => v.fold_with(&mut Normalizer {
                            drop_span: true,
                            is_test262: false,
                        }),
                        Err(err) => {
                            if err.to_string().contains("invalid type: null, expected f64") {
                                return Ok(());
                            }

                            panic!(
                                "failed to deserialize json back to module: {}\n{}",
                                err, json
                            )
                        }
                    };

                    assert_eq!(module, deser, "JSON:\n{}", json);

                    Ok(())
                })
                .unwrap();
            }
        });
    }

    Ok(())
}

fn with_parser<F, Ret>(
    treat_error_as_bug: bool,
    file_name: &Path,
    no_early_errors: bool,
    f: F,
) -> Result<Ret, StdErr>
where
    F: FnOnce(&mut Parser<Lexer<StringInput<'_>>>) -> PResult<Ret>,
{
    let fname = file_name.display().to_string();
    let output = ::testing::run_test(treat_error_as_bug, |cm, handler| {
        let fm = cm
            .load_file(file_name)
            .unwrap_or_else(|e| panic!("failed to load {}: {}", file_name.display(), e));

        let lexer = Lexer::new(
            Syntax::Typescript(TsConfig {
                dts: fname.ends_with(".d.ts"),
                tsx: fname.contains("tsx"),
                dynamic_import: true,
                decorators: true,
                import_assertions: true,
                no_early_errors,
                ..Default::default()
            }),
            JscTarget::Es2015,
            (&*fm).into(),
            None,
        );

        let mut p = Parser::new_from(lexer);

        let res = f(&mut p).map_err(|e| e.into_diagnostic(&handler).emit());

        for err in p.take_errors() {
            err.into_diagnostic(&handler).emit();
        }

        if handler.has_errors() {
            return Err(());
        }

        res
    });

    output
}

#[test]
fn references() {
    let args: Vec<_> = env::args().collect();
    let mut tests = Vec::new();
    reference_tests(&mut tests, false).unwrap();
    test_main(&args, tests, Some(Options::new()));
}

#[test]
fn errors() {
    let args: Vec<_> = env::args().collect();
    let mut tests = Vec::new();
    reference_tests(&mut tests, true).unwrap();
    test_main(&args, tests, Some(Options::new()));
}

fn is_backtrace_enabled() -> bool {
    match ::std::env::var("RUST_BACKTRACE") {
        Ok(val) => val == "1" || val == "full",
        _ => false,
    }
}
