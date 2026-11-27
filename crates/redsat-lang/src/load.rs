//! Loading a program's files: the main file, the standard library prelude, and imports.
//!
//! All files are parsed into one program: the prelude's functions first, then the declarations
//! of imported files (transitively, each file once), then the main file. Declarations are
//! hoisted, so the order only matters for the main file's statements. Imported files may only
//! declare functions, enums and structs (and import further files); their paths are resolved
//! relative to the file that imports them. A function you define (in the main file or an
//! imported one) shadows a prelude function with the same name.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ast::{Program, Stmt, StmtKind};
use crate::diagnostic::Diagnostic;
use crate::parser;
use crate::source::{SourceFile, SourceMap, Span};

/// The standard library prelude, written in RedSAT.
pub const PRELUDE: &str = include_str!("prelude.rsat");

/// The name the prelude is shown under in diagnostics.
pub const PRELUDE_NAME: &str = "<prelude>";

/// Parses the main file, the prelude and every imported file. Returns the source map (always,
/// so that errors can be rendered) and the merged program, or the errors that prevented it.
pub(crate) fn load(main: &SourceFile) -> (SourceMap, Result<Program, Vec<Diagnostic>>) {
    let mut sources = SourceMap::new(main.clone());
    let main_program = match parser::parse_from(sources.main(), 0) {
        Ok(program) => program,
        Err(error) => return (sources, Err(vec![error])),
    };
    let mut next_decl = main_program.num_decls;
    let mut stmts = Vec::new();
    let mut errors = Vec::new();

    let prelude = sources.add(SourceFile::new(PRELUDE_NAME, PRELUDE)).clone();
    match parser::parse_from(&prelude, next_decl) {
        Ok(program) => {
            next_decl = program.num_decls;
            stmts.extend(program.stmts);
        }
        Err(error) => errors.push(error),
    }

    let mut loaded: Vec<PathBuf> = main
        .path()
        .and_then(|path| path.canonicalize().ok())
        .into_iter()
        .collect();
    let mut pending = imports_of(&main_program.stmts, main.path());
    while let Some((directory, target, span)) = pending.pop() {
        let Some(directory) = directory else {
            errors.push(
                Diagnostic::error(span, format!("cannot import `{target}` here"))
                    .with_note("imports are resolved relative to the program's file, and this program is not a file"),
            );
            continue;
        };
        let path = directory.join(&target);
        let canonical = match path.canonicalize() {
            Ok(canonical) => canonical,
            Err(error) => {
                errors.push(
                    Diagnostic::error(
                        span,
                        format!("cannot find `{target}`: {}", describe_io(&error)),
                    )
                    .with_label("imported here"),
                );
                continue;
            }
        };
        if loaded.contains(&canonical) {
            continue;
        }
        loaded.push(canonical.clone());
        let text = match std::fs::read_to_string(&canonical) {
            Ok(text) => text,
            Err(error) => {
                errors.push(Diagnostic::error(
                    span,
                    format!("cannot read `{target}`: {}", describe_io(&error)),
                ));
                continue;
            }
        };
        let file = sources
            .add(SourceFile::new(target.clone(), text).with_path(canonical.clone()))
            .clone();
        match parser::parse_from(&file, next_decl) {
            Ok(module) => {
                next_decl = module.num_decls;
                for stmt in &module.stmts {
                    if !matches!(
                        stmt.kind,
                        StmtKind::Fn(_)
                            | StmtKind::Enum(_)
                            | StmtKind::Struct(_)
                            | StmtKind::Import(_)
                    ) {
                        errors.push(
                            Diagnostic::error(
                                stmt.span,
                                "an imported file may only declare functions, enums and structs",
                            )
                            .with_related(span, "imported here"),
                        );
                    }
                }
                pending.extend(imports_of(&module.stmts, Some(&canonical)));
                stmts.extend(module.stmts);
            }
            Err(error) => errors.push(error),
        }
    }

    stmts.extend(main_program.stmts);
    // Your own functions shadow the prelude's, so adding to the standard library never breaks
    // an existing program. Shadowing is lexical: the library's own functions keep calling the
    // library's versions, which are renamed to names you cannot write.
    let own: HashMap<String, String> = stmts
        .iter()
        .filter(|stmt| !prelude.contains(stmt.span.start))
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Fn(decl) => {
                Some((decl.name.name.clone(), format!("std::{}", decl.name.name)))
            }
            _ => None,
        })
        .collect();
    if !own.is_empty() {
        for stmt in stmts
            .iter_mut()
            .filter(|stmt| prelude.contains(stmt.span.start))
        {
            if let StmtKind::Fn(decl) = &mut stmt.kind {
                let decl = Rc::make_mut(decl);
                if let Some(renamed) = own.get(&decl.name.name) {
                    decl.name.name.clone_from(renamed);
                }
                crate::rename::calls_in_block(&mut decl.body, &own);
            }
        }
    }
    let library = Some(Span::new(
        prelude.base(),
        prelude.base() + prelude.text().len(),
    ));
    if errors.is_empty() {
        (
            sources,
            Ok(Program {
                stmts,
                num_decls: next_decl,
                library,
            }),
        )
    } else {
        (sources, Err(errors))
    }
}

/// An I/O error without platform-specific wording (such as "os error 2").
fn describe_io(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "no such file".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        kind => kind.to_string(),
    }
}

/// The `import` statements of a file: the directory to resolve them in, the target and where.
fn imports_of(stmts: &[Stmt], path: Option<&Path>) -> Vec<(Option<PathBuf>, String, Span)> {
    let directory = path.map(|path| {
        path.parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });
    stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Import(target) => Some((directory.clone(), target.clone(), stmt.span)),
            _ => None,
        })
        .collect()
}
