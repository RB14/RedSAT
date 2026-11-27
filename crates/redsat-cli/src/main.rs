//! `redsat`: the RedSAT command-line tool.

use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use redsat_sat::{Cadical, SolveResult, Solver, dimacs};

/// RedSAT: declarative programming by reduction to SAT.
#[derive(Parser)]
#[command(name = "redsat", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Solve a CNF formula in DIMACS format and print the answer in SAT Competition format.
    ///
    /// Exits with 10 when satisfiable and 20 when unsatisfiable, as SAT solvers do.
    Dimacs {
        /// The `.cnf` file to solve, or `-` to read standard input.
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Dimacs { path } => solve_dimacs(&path),
    };
    result.unwrap_or_else(|error| {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    })
}

fn solve_dimacs(path: &Path) -> Result<ExitCode, Box<dyn Error>> {
    let cnf = if path == Path::new("-") {
        dimacs::parse(io::stdin().lock())?
    } else {
        let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        dimacs::parse(BufReader::new(file))?
    };

    let mut solver = Cadical::new();
    let vars = cnf.load_into(&mut solver);
    let result = solver.solve();

    let mut out = BufWriter::new(io::stdout().lock());
    writeln!(out, "s {result}")?;
    if result == SolveResult::Sat {
        // Model lines in the original numbering: "v 1 -2 3 ... 0", wrapped for readability.
        let model: Vec<String> = vars
            .iter()
            .zip(1i64..)
            .map(|(var, number)| {
                if solver.value(var.pos()) {
                    number
                } else {
                    -number
                }
            })
            .map(|lit| lit.to_string())
            .chain(["0".to_string()])
            .collect();
        for line in model.chunks(20) {
            writeln!(out, "v {}", line.join(" "))?;
        }
    }
    out.flush()?;

    Ok(ExitCode::from(match result {
        SolveResult::Sat => 10,
        SolveResult::Unsat => 20,
        SolveResult::Unknown => 0,
    }))
}
