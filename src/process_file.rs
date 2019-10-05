//
// process_file.rs
// Copyright (C) 2019 seitz_local <seitz_local@lmeXX>
// Distributed under terms of the GPLv3 license.
//
use crate::beamer::get_frames;
use crate::parsing;

use log::Level::Trace;

use cachedir::CacheDirConfig;
use clap::ArgMatches;
use latexcompile::{LatexCompiler, LatexInput, LatexRunOptions};
use md5;
use rayon;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashMap;
use std::fs::write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::vec::Vec;

lazy_static! {
    static ref FRAME_REGEX: Regex =
        Regex::new(r"(?ms)^\\begin\{frame\}.*?^\\end\{frame\}").unwrap();
}
lazy_static! {
    static ref DOCUMENT_REGEX: Regex =
        Regex::new(r"(?ms)^\\begin\{document\}.*^\\end\{document\}").unwrap();
}

lazy_static! {
    static ref PREVIOUS_FRAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

pub fn process_file(input_file: &str, args: &ArgMatches) {
    let input_path = Path::new(&input_file);
    if !input_path.is_file() {
        eprintln!("Could not open {}", input_file);
        return;
    }

    let parsed_file = parsing::ParsedFile::new(input_file.to_string());
    debug!("{}", parsed_file.syntax_tree.root_node().to_sexp());

    let frame_nodes = get_frames(&parsed_file);
    info!("Found {} frames with tree-sitter.", frame_nodes.len());

    let mut frames = Vec::with_capacity(frame_nodes.len());

    if !frame_nodes.is_empty() {
        for f in frame_nodes.iter() {
            let node_string = parsed_file.get_node_string(&f);
            frames.push(node_string.to_string());
        }
    } else {
        for cap in FRAME_REGEX.captures_iter(&parsed_file.file_content) {
            let frame_string = cap[0].to_string();
            frames.push(frame_string);
        }
    }
    info!("Found {} frames.", frames.len());

    if log_enabled!(Trace) {
        let root_node = parsed_file.syntax_tree.root_node();
        let mut stack = vec![root_node];

        while !stack.is_empty() {
            let current_node = stack.pop().unwrap();
            if current_node.kind() == "ERROR" {
                eprintln!(
                    "\n{}:\n\t {}",
                    current_node.kind(),
                    parsed_file.get_node_string(&current_node),
                );
            }

            for i in (0..current_node.named_child_count()).rev() {
                stack.push(current_node.named_child(i).unwrap());
            }
        }
    }

    //let document_env = tree_traversal::get_children(
    //parsed_file.syntax_tree.root_node(),
    //&|n| n.kind() == "document_env",
    //true,
    //TraversalOrder::BreadthFirst,
    //);
    //let preamble =[> if document_env.len() == 1 as usize {<]
    //parsed_file.file_content[0..document_env[0].start_byte()].to_owned()
    //} else {
    //warn!(
    //"Could not find document environment with tree_sitter ({})",
    //input_file
    /*);*/
    let find = parsed_file.file_content.find("\\begin{document}");
    let preamble = match find {
        Some(x) => Some(parsed_file.file_content[..x].to_owned()),
        None => None,
    }
    .unwrap();

    let cachedir: PathBuf = CacheDirConfig::new("faster-beamer")
        .get_cache_dir()
        .unwrap()
        .into();

    let preamble_hash = md5::compute(&preamble);
    let preamble_filename = format!("{:x}_{}", preamble_hash, args.is_present("draft"));
    if input_path
        .parent()
        .unwrap()
        .join(format!("{}.fmt", preamble_filename))
        .is_file()
    {
        info!("Precompiled preamble already exists");
    } else {
        info!(
            "Precompiling preamble {:?}",
            input_path.join(format!("{}.fmt", preamble_filename))
        );
        let output = Command::new("pdflatex")
            .arg("-shell-escape")
            .arg("-ini")
            .arg(format!("-jobname=\"{}\"", preamble_filename))
            .arg("\"&pdflatex\"")
            .arg("mylatexformat.ltx")
            .arg(&input_file)
            .output()
            .expect("Failed to compile preamble");
        //eprint!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    let mut generated_documents = Vec::new();
    let mut command = &mut Command::new("pdfunite");
    for f in &frames {
        let compile_string = format!("%&{}\n", preamble_filename)
            + &preamble
            + "\n\\begin{document}\n"
            + &f
            + "\n\\end{document}\n";

        let hash = md5::compute(&compile_string);
        let output = cachedir.join(format!("{:x}.pdf", hash));
        generated_documents.push((hash, compile_string));

        command = command.arg(output.to_str().unwrap());
    }

    trace!("Comparing frames");
    let mut first_changed_frame = 0;
    for frame_pair in frames.iter().zip((*PREVIOUS_FRAMES.lock().unwrap()).iter()) {
        match frame_pair {
            (lhs, rhs) if lhs != rhs => {
                dbg!("{:?} vs {:?}", lhs, rhs);
                break;
            }
            _ => first_changed_frame += 1,
        }
    }
    debug!(
        "Found first difference in frame {} from {}",
        &first_changed_frame,
        frames.len()
    );

    generated_documents
        .par_iter()
        .for_each(|(hash, tex_content)| {
            let pdf = cachedir.join(format!("{:x}.pdf", hash));

            if pdf.is_file() {
                debug!("{} is already compiled!", pdf.to_str().unwrap_or("???"));
            } else {
                let temp_file = cachedir.join(format!("{:x}.tex", hash));
                assert!(write(&temp_file, &tex_content).is_ok());
                let dict = HashMap::new();
                let compiler = LatexCompiler::new(dict)
                    .unwrap()
                    .add_arg("-shell-escape")
                    .add_arg("-interaction=nonstopmode");

                let latex_input = LatexInput::from(input_path.parent().unwrap().to_str().unwrap());
                let result = compiler.run(
                    &temp_file.to_string_lossy(),
                    &latex_input,
                    LatexRunOptions::new(),
                );
                if result.is_ok() {
                    assert!(write(&pdf, &result.unwrap()).is_ok());
                    info!("Compiled file {}", &temp_file.to_str().unwrap())
                } else {
                    error!("Failed to compile frame ({})", &temp_file.to_str().unwrap());
                    error!("{:?}", result.err());
                };
            };
        });

    let output_file = args.value_of("OUTPUT").unwrap_or("output.pdf");

    if args.is_present("unite") {
        info!("PDF unite!");
        let output = command
            .arg(output_file)
            .output()
            .expect("failed to execute process");

        eprint!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        if first_changed_frame < generated_documents.len() {
            let (hash, _) = generated_documents[first_changed_frame];
            let compiled_pdf = cachedir.join(format!("{:x}.pdf", hash));
            info!("Linking: {:?} -> {:?}", &compiled_pdf, &output_file);
            if Path::new(output_file).is_file() {
                let _result = ::std::fs::remove_file(&output_file).expect("Tried to delete previous output file");
            }
            ::symlink::symlink_file(compiled_pdf, output_file).expect("Failed to create symlink to output file.");
        }
    }

    *PREVIOUS_FRAMES.lock().unwrap() = frames;
}
