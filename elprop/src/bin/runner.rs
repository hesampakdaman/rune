mod code;
use code::output::{Output, Status};
use proptest::prelude::TestCaseError;
use proptest::test_runner::{Config, TestError, TestRunner};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::{fs, path::PathBuf};

const START_TAG: &str = ";; ELPROP_START:";
const END_TAG: &str = "\n;; ELPROP_END\n";

fn main() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_root.parent().unwrap();
    let target = workspace_root.join("target/elprop");

    let json = target.join("functions.json");
    // read the file to a string
    let json_string = fs::read_to_string(json).expect("Unable to read file");

    let config: code::data::Config =
        serde_json::from_str(&json_string).expect("Unable to deserialize json");

    let mut runner = TestRunner::new(Config {
        cases: config.test_count,
        failure_persistence: None,
        ..Config::default()
    });

    let cmd = crate_root.parent().unwrap().join("target/debug/rune");
    #[expect(clippy::zombie_processes)]
    let mut child = std::process::Command::new(cmd)
        .arg("--eval-stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start rune");

    let rune_stdin = RefCell::new(child.stdin.take().unwrap());
    let rune_stdout = RefCell::new(child.stdout.take().unwrap());
    let master_count = RefCell::new(0);

    let outputs = RefCell::new(Vec::new());
    for func in config.functions {
        let name = func.name.clone();
        let result = runner.run(&func.strategy(), |input| {
            let body = code::data::print_args(&input);
            // send to emacs
            println!(";; sending to Emacs");
            let test_str = format!(";; ELPROP_START\n({name} {body})\n;; ELPROP_END");
            println!("{test_str}");
            // send to rune
            println!(";; sending to rune");
            writeln!(rune_stdin.borrow_mut(), "{test_str}").unwrap();

            let mut reader = BufReader::new(std::io::stdin());
            println!(";; reading from Emacs");
            let emacs_output =
                process_eval_result("Emacs", *master_count.borrow(), &mut reader, |_| {});

            let mut rune_stdout = rune_stdout.borrow_mut();
            let mut reader = BufReader::new(&mut *rune_stdout);
            println!(";; reading from Rune");
            let rune_output =
                process_eval_result("Rune", *master_count.borrow(), &mut reader, |x| {
                    assert!(
                        !(x.contains("\nError: ") || x.starts_with("Error: ")),
                        "Rune Error: {x}"
                    );
                });
            println!(";; done");

            *master_count.borrow_mut() += 1;

            if emacs_output == rune_output {
                Ok(())
            } else {
                println!("\"Emacs: '{emacs_output}', Rune: '{rune_output}'\"");
                Err(TestCaseError::Fail(
                    format!("Emacs: {emacs_output}, Rune: {rune_output}").into(),
                ))
            }
        });

        // send the output of "result" to a file
        // open the file in write mode

        println!(";; sending output");
        let status = match result {
            Err(TestError::Fail(reason, value)) => Status::Fail(reason.to_string(), value),
            Err(TestError::Abort(reason)) => Status::Abort(reason.to_string()),
            Ok(()) => Status::Pass,
        };
        let output = Output { function: func.name.clone(), status };
        outputs.borrow_mut().push(output);
    }

    let _ = child.kill();
    let json = serde_json::to_string(&*outputs.borrow()).expect("Malformed Output JSON");
    let output_file = target.join("output.json");
    fs::write(output_file, json).unwrap();
    println!(";; exit process");
}

fn process_eval_result(
    name: &str,
    master_count: usize,
    reader: &mut impl BufRead,
    test_fail: impl Fn(&str),
) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    test_fail(&line);
    let count = line.strip_prefix(START_TAG).unwrap().trim().parse::<usize>().unwrap();
    assert_eq!(
        master_count, count,
        "Count from {name} was off. actual {count}, expected {master_count}",
    );
    line.clear();
    while !line.ends_with(END_TAG) {
        reader.read_line(&mut line).unwrap();
    }
    line.strip_suffix(END_TAG).unwrap().trim().to_string()
}