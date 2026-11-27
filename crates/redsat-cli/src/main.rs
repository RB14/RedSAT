//! `redsat`: the RedSAT command-line tool.
//!
//! ```text
//! redsat prog.rsat [options]          run a program (the default action)
//! redsat check prog.rsat [options]    parse and type-check only
//! redsat dimacs problem.cnf           solve a raw DIMACS file
//! ```

use std::error::Error;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use redsat_lang::config::{find_config, load_config};
use redsat_lang::{
    CompilerOptions, Diagnostic, ExplicitTypes, OptionLayer, SourceFile, UnboundedInts,
};
use redsat_sat::{Cadical, SolveResult, Solver, dimacs};

/// RedSAT: declarative programming by reduction to SAT.
///
/// Give it a `.rsat` file to run, or use one of the commands below.
#[derive(Parser)]
#[command(
    name = "redsat",
    version,
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// The `.rsat` program to run.
    #[arg(required = true)]
    file: Option<PathBuf>,

    #[command(flatten)]
    options: OptionArgs,

    /// Print per-solve statistics (domains, bit widths, circuit and CNF sizes, time) to stderr.
    #[arg(long)]
    stats: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and type-check a program without running it.
    Check {
        /// The `.rsat` program to check.
        file: PathBuf,

        #[command(flatten)]
        options: OptionArgs,
    },
    /// Solve a CNF formula in DIMACS format and print the answer in SAT Competition format.
    ///
    /// Exits with 10 when satisfiable and 20 when unsatisfiable, as SAT solvers do.
    Dimacs {
        /// The `.cnf` file to solve, or `-` to read standard input.
        path: PathBuf,
    },
}

/// Compiler options; they override `redsat.toml`.
#[derive(Args, Default)]
struct OptionArgs {
    /// Enable every strict check.
    #[arg(long)]
    strict: bool,

    /// Which declarations need a type annotation.
    #[arg(long, value_name = "off|unknowns|all", value_parser = parse::<ExplicitTypes>)]
    explicit_types: Option<ExplicitTypes>,

    /// What to do with integers whose bounds cannot be inferred.
    #[arg(long, value_name = "deepen|error", value_parser = parse::<UnboundedInts>)]
    unbounded_ints: Option<UnboundedInts>,

    /// First magnitude (in bits) tried for unbounded integers.
    #[arg(long, value_name = "BITS")]
    fallback_int_bits: Option<u32>,

    /// Stop widening unbounded integers beyond this many bits.
    #[arg(long, value_name = "BITS")]
    max_int_bits: Option<u32>,

    /// Use this configuration file instead of searching for `redsat.toml`.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

fn parse<T: std::str::FromStr<Err = String>>(text: &str) -> Result<T, String> {
    text.parse()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Command::Dimacs { path }) => solve_dimacs(&path),
        Some(Command::Check { file, options }) => check(&file, &options),
        None => run(
            &cli.file.expect("required by clap"),
            &cli.options,
            cli.stats,
        ),
    };
    result.unwrap_or_else(|error| {
        eprintln!("error: {error}");
        ExitCode::FAILURE
    })
}

/// Reads the program and resolves the options that apply to it.
fn prepare(
    file: &Path,
    args: &OptionArgs,
) -> Result<(SourceFile, CompilerOptions), Box<dyn Error>> {
    let text =
        std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let directory = file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let config_path = args
        .config
        .clone()
        .or_else(|| find_config(&directory.canonicalize().unwrap_or(directory)));
    let file_layer = match &config_path {
        Some(path) => load_config(path)?,
        None => OptionLayer::default(),
    };
    let cli_layer = OptionLayer {
        strict: args.strict.then_some(true),
        explicit_types: args.explicit_types,
        unbounded_ints: args.unbounded_ints,
        fallback_int_bits: args.fallback_int_bits,
        max_int_bits: args.max_int_bits,
    };
    let options = CompilerOptions::resolve(&file_layer, &cli_layer)?;
    Ok((SourceFile::new(file.display().to_string(), text), options))
}

fn report(source: &SourceFile, diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprint!("{}", diagnostic.render(source));
    }
}

fn run(file: &Path, args: &OptionArgs, stats: bool) -> Result<ExitCode, Box<dyn Error>> {
    let (source, options) = prepare(file, args)?;
    let factory = || Box::new(Cadical::new()) as Box<dyn Solver>;
    let mut out = io::stdout();
    let outcome = redsat_lang::run(&source, &options, &factory, &mut out);
    out.flush()?;
    report(&source, &outcome.diagnostics);
    if stats {
        for solve_stats in &outcome.stats {
            eprint!("{}", solve_stats.render(&source));
        }
    }
    Ok(if outcome.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn check(file: &Path, args: &OptionArgs) -> Result<ExitCode, Box<dyn Error>> {
    let (source, options) = prepare(file, args)?;
    let diagnostics = redsat_lang::check(&source, &options);
    report(&source, &diagnostics);
    if diagnostics.iter().any(Diagnostic::is_error) {
        Ok(ExitCode::FAILURE)
    } else {
        eprintln!("{}: ok", source.name());
        Ok(ExitCode::SUCCESS)
    }
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
