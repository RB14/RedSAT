//! `redsat repl`: build a program one statement at a time.
//!
//! Each accepted input is appended to the session's program. To evaluate an input, the whole
//! program (history plus the new input) is run, and only the output beyond what the history
//! printed is shown. Runs are deterministic, so that prefix is stable. An input with errors is
//! rejected and leaves the history unchanged. An input that is a bare expression is printed.

use std::io::{self, BufRead, Write};

use redsat_lang::{CompilerOptions, Severity, SolverFactory, SourceFile};

const HELP: &str = "\
Enter RedSAT statements; they accumulate into a program. A bare expression is printed.
Blocks may span several lines: input continues while brackets are open.

  :help          this help
  :show          print the program so far
  :undo          remove the last accepted input
  :reset         start over
  :load <file>   append a file's contents
  :quit          leave (also Ctrl-D)";

/// Runs the REPL on stdin/stdout.
pub fn run(options: &CompilerOptions, factory: &SolverFactory) -> io::Result<()> {
    println!(
        "RedSAT {} — type :help for help, :quit to leave",
        env!("CARGO_PKG_VERSION")
    );
    let mut history: Vec<String> = Vec::new();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        let Some(input) = read_input(&mut lines)? else {
            println!();
            return Ok(());
        };
        let trimmed = input.trim();
        match trimmed.split_whitespace().next().unwrap_or("") {
            "" => continue,
            ":quit" | ":q" | ":exit" => return Ok(()),
            ":help" | ":h" => println!("{HELP}"),
            ":show" => print!("{}", history.concat()),
            ":reset" => {
                history.clear();
                println!("(program cleared)");
            }
            ":undo" => {
                if history.pop().is_some() {
                    println!("(removed the last input)");
                }
            }
            ":load" => {
                let path = trimmed[":load".len()..].trim();
                match std::fs::read_to_string(path) {
                    Ok(text) => evaluate(&mut history, &text, options, factory),
                    Err(error) => eprintln!("error: {path}: {error}"),
                }
            }
            command if command.starts_with(':') => {
                eprintln!("unknown command {command}; try :help")
            }
            _ => evaluate(&mut history, &input, options, factory),
        }
    }
}

/// Reads one input: a line, continued while brackets are open. `None` at end of input.
fn read_input(lines: &mut impl Iterator<Item = io::Result<String>>) -> io::Result<Option<String>> {
    let mut input = String::new();
    loop {
        print!("{}", if input.is_empty() { "rsat> " } else { "  ... " });
        io::stdout().flush()?;
        let Some(line) = lines.next() else {
            return Ok(if input.is_empty() { None } else { Some(input) });
        };
        input.push_str(&line?);
        input.push('\n');
        if open_brackets(&input) <= 0 {
            return Ok(Some(input));
        }
    }
}

/// How many brackets are open at the end of `text` (ignoring strings and comments).
fn open_brackets(text: &str) -> i32 {
    let mut depth = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '"' => {
                while let Some(s) = chars.next() {
                    match s {
                        '\\' => {
                            chars.next();
                        }
                        '"' | '\n' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for s in chars.by_ref() {
                    if s == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    depth
}

/// Runs the history plus `input`; on success shows the new output and keeps `input`.
fn evaluate(
    history: &mut Vec<String>,
    input: &str,
    options: &CompilerOptions,
    factory: &SolverFactory,
) {
    let previous = history.concat();
    let (before, _) = execute(&previous, options, factory);
    let attempt = format!("{previous}{input}");
    let (output, errors) = execute(&attempt, options, factory);
    let accepted = if errors.is_empty() {
        Some((input.to_string(), output))
    } else {
        // A bare expression is shown with `print`.
        let printed = format!("print {}\n", input.trim());
        let (output, print_errors) = execute(&format!("{previous}{printed}"), options, factory);
        if print_errors.is_empty() {
            Some((printed, output))
        } else {
            None
        }
    };
    match accepted {
        Some((kept, output)) => {
            print!("{}", output.get(before.len()..).unwrap_or(""));
            history.push(kept);
        }
        None => {
            let (_, errors) = execute(&attempt, options, factory);
            for error in errors {
                eprint!("{error}");
            }
        }
    }
}

/// Runs `text`; returns its output (with warnings and notes) and its rendered errors.
fn execute(
    text: &str,
    options: &CompilerOptions,
    factory: &SolverFactory,
) -> (String, Vec<String>) {
    let source = SourceFile::new("<repl>", text);
    let mut out = Vec::new();
    let outcome = redsat_lang::run(&source, options, factory, &mut out);
    let mut output = String::from_utf8_lossy(&out).into_owned();
    let mut errors = Vec::new();
    for diagnostic in &outcome.diagnostics {
        if diagnostic.severity == Severity::Error {
            errors.push(diagnostic.render(&outcome.sources));
        } else {
            output.push_str(&diagnostic.render(&outcome.sources));
        }
    }
    (output, errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_open_brackets_outside_strings_and_comments() {
        assert_eq!(open_brackets("solve x {\n"), 1);
        assert_eq!(open_brackets("print \"{\" // {\n"), 0);
        assert_eq!(open_brackets("var a = [1,\n2]\n"), 0);
    }
}
