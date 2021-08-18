extern crate serde;
extern crate serde_json;

use num_bigint::BigUint;
use std::fs::{File, create_dir_all};
use std::io::{copy, stdout, stdin, BufReader};
use std::path::{Path, PathBuf};
use structopt::clap::AppSettings::*;
pub use structopt::StructOpt;

use crate::consumers::{
    evaluator::Evaluator,
    source::{has_sieve_extension, list_workspace_files},
    stats::Stats,
    validator::Validator,
};
use crate::producers::from_r1cs::R1CSConverter;
use crate::{Messages, Result, Source};

const ABOUT: &str = "
This is a collection of tools to work with zero-knowledge statements encoded in SIEVE IR messages.

The tools below work within a workspace directory given after the tool name (`workspace` in the examples below), or in the current working directory by default. To read from stdin or write to stdout, pass a dash - instead of a filename.

Create an example statement:
    zki_sieve example workspace

Print a statement in different forms:
    zki_sieve to-text workspace
    zki_sieve to-json workspace
    zki_sieve to-yaml workspace

Validate and evaluate a proving system:
    zki_sieve valid-eval-metrics workspace

";

#[derive(Debug, StructOpt)]
#[structopt(
name = "zki_sieve",
about = "zkInterface toolbox for SIEVE IR.",
long_about = ABOUT,
setting(DontCollapseArgsInUsage),
setting(ColoredHelp)
)]
pub struct Options {
    /// Which tool to run.
    ///
    /// example       Produce example statements.
    ///
    /// to-text       Print the content in a human-readable form.
    ///
    /// to-json       Convert to JSON on a single line.
    ///
    /// to-yaml       Convert to YAML.
    ///
    /// validate      Validate the format and semantics of a statement, as seen by a verifier.
    ///
    /// evaluate      Evaluate a circuit as prover to check that the statement is true, i.e. the witness satisfies the circuit.
    ///
    /// metrics       Calculate statistics about the circuit.
    ///
    /// valid-eval-metrics    Combined validate, evaluate, and metrics.
    ///
    /// zkif-to-ir    Convert zkinterface files into SIEVE IR.
    ///
    /// ir-to-zkif    Convert SIEVE IR files into R1CS zkinterface.
    ///
    /// flatten       Flatten a SIEVE IR relation.
    ///
    /// list-validations    Lists all the checks performed by the validator.
    ///
    /// cat           Concatenate .sieve files to stdout to pipe to another program.
    #[structopt(default_value = "help")]
    pub tool: String,

    /// The tools work in a workspace directory containing .sieve files.
    ///
    /// Alternatively, a list of .sieve files can be provided explicitly.
    ///
    /// The dash - means either write to stdout or read from stdin.
    #[structopt(default_value = ".")]
    pub paths: Vec<PathBuf>,

    /// Which field to use when generating circuits.
    #[structopt(short, long, default_value = "101")]
    pub field_order: BigUint,

    /// `example --incorrect` will generate an incorrect witness useful for negative tests.
    #[structopt(long)]
    pub incorrect: bool,

    ///
    #[structopt(short, long, default_value = "-")]
    pub resource: String,

    /// `ir-to-zkif --modular-reduce` will produce zkinterface R1CS with baked-in modular reduction (because libsnark does not respect field size).
    #[structopt(long)]
    pub modular_reduce: bool,

    /// Which directory to use when simplifying circuits.
    #[structopt(short, long, default_value = "-")]
    pub out: PathBuf,

    /// First available temporary wire id to use when simplifying circuits (2^63 if unspecified).
    #[structopt(long)]
    pub tmp_wire_start: Option<u64>,

}

pub fn cli(options: &Options) -> Result<()> {
    match &options.tool[..] {
        "example" => main_example(options),
        "to-text" => main_text(&load_messages(options)?),
        "to-json" => main_json(&load_messages(options)?),
        "from-json" => from_json(options),
        "to-yaml" => main_yaml(&load_messages(options)?),
        "from-yaml" => from_yaml(options),
        "validate" => main_validate(&stream_messages(options)?),
        "evaluate" => main_evaluate(&stream_messages(options)?),
        "metrics" => main_metrics(&stream_messages(options)?),
        "valid-eval-metrics" => main_valid_eval_metrics(&stream_messages(options)?),
        "zkif-to-ir" => main_zkif_to_ir(options),
        "ir-to-zkif" => main_ir_to_r1cs(options),
        "flatten" => main_ir_flattening(options),
        "list-validations" => main_list_validations(),
        "cat" => main_cat(options),
        "simulate" => Err("`simulate` was renamed to `evaluate`".into()),
        "stats" => Err("`stats` was renamed to `metrics`".into()),
        "help" => {
            Options::clap().print_long_help()?;
            eprintln!("\n");
            Ok(())
        }
        _ => {
            Options::clap().print_long_help()?;
            eprintln!("\n");
            Err(format!("Unknown command {}", &options.tool).into())
        }
    }
}

fn load_messages(opts: &Options) -> Result<Messages> {
    stream_messages(opts)?.read_all_messages()
}

fn stream_messages(opts: &Options) -> Result<Source> {
    let mut source = Source::from_dirs_and_files(&opts.paths)?;
    source.print_filenames = true;
    Ok(source)
}

fn main_example(opts: &Options) -> Result<()> {
    use crate::producers::examples::*;
    use crate::{FilesSink, Sink};

    let header = example_header_in_field(opts.field_order.to_bytes_le());
    let instance = example_instance_h(&header);
    let relation = example_relation_h(&header);
    let witness = if opts.incorrect {
        example_witness_incorrect_h(&header)
    } else {
        example_witness_h(&header)
    };

    if opts.paths.len() != 1 {
        return Err("Specify a single directory where to write examples.".into());
    }
    let out_dir = &opts.paths[0];

    if out_dir == Path::new("-") {
        instance.write_into(&mut stdout())?;
        witness.write_into(&mut stdout())?;
        relation.write_into(&mut stdout())?;
    } else if has_sieve_extension(out_dir) {
        let mut file = File::create(out_dir)?;
        instance.write_into(&mut file)?;
        witness.write_into(&mut file)?;
        relation.write_into(&mut file)?;
        eprintln!(
            "Written Instance, Witness, and Relation into {}",
            out_dir.display()
        );
    } else {
        let mut sink = FilesSink::new_clean(out_dir)?;
        sink.print_filenames();
        sink.push_instance_message(&instance)?;
        sink.push_witness_message(&witness)?;
        sink.push_relation_message(&relation)?;
    }
    Ok(())
}

fn main_cat(opts: &Options) -> Result<()> {
    for path in list_workspace_files(&opts.paths)? {
        let mut file = File::open(&path)?;
        let mut stdout = stdout();
        copy(&mut file, &mut stdout)?;
    }
    Ok(())
}

fn main_text(_messages: &Messages) -> Result<()> {
    Err("Text form is not implemented yet.".into())
}

fn main_json(messages: &Messages) -> Result<()> {
    serde_json::to_writer(stdout(), messages)?;
    println!();
    Ok(())
}

fn from_json(options: &Options) -> Result<()> {
    let messages: Messages = match &options.resource [..] {
        "-" => serde_json::from_reader(stdin())?,
        _ => {
            let file = File::open(&options.resource)?;
            let reader = BufReader::new(file);
            serde_json::from_reader(reader)?
        },
    };
    let mut file = File::create("from_json.sieve")?;
    for instance in messages.instances {
        instance.write_into(&mut file)?;
    }
    for witness in messages.witnesses {
        witness.write_into(&mut file)?;
    }
    for relation in messages.relations {
        relation.write_into(&mut file)?;
    }
    Ok(())
}

fn main_yaml(messages: &Messages) -> Result<()> {
    serde_yaml::to_writer(stdout(), messages)?;
    println!();
    Ok(())
}

fn from_yaml(options: &Options) -> Result<()> {
    let messages: Messages = match &options.resource [..] {
        "-" => serde_yaml::from_reader(stdin())?,
        _ => {
            let file = File::open(&options.resource)?;
            let reader = BufReader::new(file);
            serde_yaml::from_reader(reader)?
        },
    };
    let mut file = File::create("from_yaml.sieve")?;
    for instance in messages.instances {
        instance.write_into(&mut file)?;
    }
    for witness in messages.witnesses {
        witness.write_into(&mut file)?;
    }
    for relation in messages.relations {
        relation.write_into(&mut file)?;
    }
    Ok(())
}

fn main_list_validations() -> Result<()> {
    Validator::print_implemented_checks();
    Ok(())
}

fn main_validate(source: &Source) -> Result<()> {
    // Validate semantics as verifier.
    let mut validator = Validator::new_as_prover();
    for msg in source.iter_messages() {
        validator.ingest_message(&msg?);
    }
    print_violations(
        &validator.get_violations(),
        "COMPLIANT with the specification",
    )
}

fn main_evaluate(source: &Source) -> Result<()> {
    // Validate semantics as verifier.
    let mut evaluator = Evaluator::default();
    for msg in source.iter_messages() {
        evaluator.ingest_message(&msg?);
    }
    print_violations(&evaluator.get_violations(), "TRUE")
}

fn main_metrics(source: &Source) -> Result<()> {
    let mut stats = Stats::default();
    for msg in source.iter_messages() {
        stats.ingest_message(&msg?);
    }
    serde_json::to_writer_pretty(stdout(), &stats)?;
    println!();
    Ok(())
}

/// Joint validate, evaluate, and metrics.
fn main_valid_eval_metrics(source: &Source) -> Result<()> {
    // Validate semantics as prover.
    let mut validator = Validator::new_as_prover();
    // Check whether the statement is true.
    let mut evaluator = Evaluator::default();
    // Measure metrics on the circuit.
    let mut stats = Stats::default();

    // Feed messages to all consumers (read files or stdin only once).
    for msg in source.iter_messages() {
        let msg = msg?;
        validator.ingest_message(&msg);
        evaluator.ingest_message(&msg);
        stats.ingest_message(&msg);
    }

    let res1 = print_violations(
        &validator.get_violations(),
        "COMPLIANT with the specification",
    );
    let res2 = print_violations(&evaluator.get_violations(), "TRUE");
    let res3 = serde_json::to_writer_pretty(stdout(), &stats);
    println!();

    res1?;
    res2?;
    res3?;
    Ok(())
}

fn main_zkif_to_ir(opts: &Options) -> Result<()> {
    use zkinterface::{Workspace, Message};
    use zkinterface::consumers::validator::Validator;

    use crate::FilesSink;

    // Load and validate zkinterface input
    let workspace = Workspace::from_dirs_and_files(&opts.paths)?;
    {
        // enclosed in bracket to free the potential memory hold by the ZKIF validator.
        let mut validator = Validator::new_as_verifier();
        for msg in workspace.iter_messages() {
            validator.ingest_message(&msg);
        }
        print_violations(
            &validator.get_violations(),
            "COMPLIANT with the zkinterface specification"
        )?;
    }

    // Convert to SIEVE IR

    // get the first header in the workspace
    // NB: the successful call to the validator above states that a header exist (and if many, are coherent)
    //     so unwrapping is safe.
    let zki_header = workspace
        .iter_messages()
        .find_map(|mess| match mess {
            Message::Header(head) => Some(head),
            _ => None,
        }).ok_or("Header not present in ZKIF workspace.")?;

    // instantiate the converter
    let mut converter = R1CSConverter::new(
        FilesSink::new_clean(&PathBuf::from(".")).unwrap(), 
        &zki_header
    );
    // Ingest all non-header messages
    for message in workspace.iter_messages() {
        match message {
            Message::ConstraintSystem(zkif_constraint) => converter.ingest_constraints(&zkif_constraint)?,
            Message::Witness(zkif_witness) => converter.ingest_witness(&zkif_witness)?,
            _ => {}
        }
    }
    converter.finish();

    Ok(())
}

// Convert to R1CS zkinterface format.
// Expects one instance, witness, and relation only.
fn main_ir_to_r1cs(opts: &Options) -> Result<()> {
    use crate::producers::to_r1cs::to_r1cs;

    let mut source = Source::from_directory(&std::env::current_dir()?)?;
    source.print_filenames = true;
    let messages = source.read_all_messages()?;

    assert_eq!(messages.instances.len(), 1);
    assert_eq!(messages.relations.len(), 1);
    assert_eq!(messages.witnesses.len(), 1);

    let instance = &messages.instances[0];
    let relation = &messages.relations[0];
    let witness = &messages.witnesses[0];

    let (zki_header, zki_r1cs, zki_witness) = to_r1cs(instance, &relation, witness, opts.modular_reduce);

    zki_header.write_into(&mut stdout())?;
    zki_r1cs.write_into(&mut stdout())?;
    zki_witness.write_into(&mut stdout())?;

    if opts.paths.len() != 1 {
        return Err("Specify a single directory to write r1cs into.".into());
    }
    let out_dir = &opts.paths[0];

    if out_dir == Path::new("-") {
        zki_header.write_into(&mut stdout())?;
        zki_witness.write_into(&mut stdout())?;
        zki_r1cs.write_into(&mut stdout())?;
    } else if zkinterface::consumers::workspace::has_zkif_extension(out_dir) {
        let mut file = File::create(out_dir)?;
        zki_header.write_into(&mut file)?;
        zki_witness.write_into(&mut file)?;
        zki_r1cs.write_into(&mut file)?;
    } else {
        create_dir_all(out_dir)?;

        let path = out_dir.join("header.zkif");
        zki_header.write_into(&mut File::create(&path)?)?;
        eprintln!("Written {}", path.display());

        let path = out_dir.join("witness.zkif");
        zki_witness.write_into(&mut File::create(&path)?)?;
        eprintln!("Written {}", path.display());

        let path = out_dir.join("constraints.zkif");
        zki_r1cs.write_into(&mut File::create(&path)?)?;
        eprintln!("Written {}", path.display());
    }
    
    Ok(())
}

// Flattens SIEVE IR format by removing loops functions and switches.
// Expects a set of dirs and files and a resource, places the flattened relations into the file or dir specified by --out.
fn main_ir_flattening(opts: &Options) -> Result<()> {
    use crate::consumers::flattening::{flatten_relation, flatten_relation_from};
    // use crate::{FilesSink, Sink};
    use crate::structs::message::Message;
    use crate::FILE_EXTENSION;

    let source  = stream_messages(opts)?;
    let out_dir = &opts.out;

    for msg in source.iter_messages() {
        match msg? {
            Message::Instance(_) => {}
            Message::Witness(_)  => {}
            Message::Relation(relation) => {
                let flattened_relation =
                    if let Some(tmp_wire_start) = opts.tmp_wire_start {
                        flatten_relation_from(&relation, tmp_wire_start)
                    } else {
                        flatten_relation(&relation)
                    };
                
                if out_dir == Path::new("-") {
                    flattened_relation.write_into(&mut stdout())?;
                } else if has_sieve_extension(&out_dir) {
                    let mut file = File::create(out_dir)?;
                    flattened_relation.write_into(&mut file)?;
                } else {
                    create_dir_all(out_dir)?;
                    let path = out_dir.join(format!("002_relation.{}", FILE_EXTENSION));
                    flattened_relation.write_into(&mut File::create(&path)?)?;
                    eprintln!("Written {}", path.display());
                    // FilesSink stuff doesn't seem to support splitting relation into several files
                    // and it forces the creation of empty witness and instance files;
                    // Not what we want and no benefit.
                    // let mut sink = FilesSink::new_clean(out_dir)?;
                    // sink.print_filenames();
                    // sink.push_relation_message(&flattened_relation)?;
                }
            }
        }
    }

    
    Ok(())
}


fn print_violations(errors: &[String], what_it_is_supposed_to_be: &str) -> Result<()> {
    eprintln!();
    if errors.len() > 0 {
        eprintln!("The statement is NOT {}!", what_it_is_supposed_to_be);
        eprintln!("Violations:\n- {}\n", errors.join("\n- "));
        Err(format!("Found {} violations.", errors.len()).into())
    } else {
        eprintln!("The statement is {}!", what_it_is_supposed_to_be);
        Ok(())
    }
}

#[test]
fn test_cli() -> Result<()> {
    use std::fs::remove_dir_all;

    let workspace = PathBuf::from("local/test_cli");
    let _ = remove_dir_all(&workspace);

    cli(&Options {
        tool: "example".to_string(),
        paths: vec![workspace.clone()],
        field_order: BigUint::from(101 as u32),
        incorrect: false,
        resource: "-".to_string(),
        modular_reduce: false,
        out: PathBuf::from("-"),
        tmp_wire_start: None,
    })?;

    cli(&Options {
        tool: "valid-eval-metrics".to_string(),
        paths: vec![workspace.clone()],
        field_order: BigUint::from(101 as u32),
        incorrect: false,
        resource: "-".to_string(),
        modular_reduce: false,
        out: PathBuf::from("-"),
        tmp_wire_start: None,
    })?;

    Ok(())
}
