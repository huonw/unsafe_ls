#![crate_name = "unsafe_ls"]
#![feature(rustc_private, slice_patterns)]
extern crate arena;
extern crate getopts;
extern crate syntax;
extern crate rustc;
extern crate rustc_back;
extern crate rustc_driver;
extern crate rustc_trans;
extern crate rustc_typeck;

use rustc::session::{self, config};
use rustc_driver::driver;
use rustc::middle::ty;
use rustc::session::search_paths::SearchPaths;
use syntax::codemap::Pos;
use std::collections::{HashSet, HashMap};
use std::env;
use std::sync::Arc;
use std::thread;
use std::path::{Path, PathBuf};

mod visitor;

static DEFAULT_LIB_DIR: &'static str = "/usr/local/lib/rustlib/x86_64-unknown-linux-gnu/lib";

fn main() {
    let mut args = env::args();
    let mut opts = getopts::Options::new();
    opts.optflag("h", "help", "show this help message");
    opts.optflag("n", "nonffi",
                 "print `unsafe`s that include non-FFI unsafe behaviours");
    opts.optflag("f", "ffi", "print `unsafe`s that do FFI calls");
    opts.optmulti("L", "library-path",
                  "directories to add to crate search path", "DIR");

    let name = args.next().unwrap();
    let matches = opts.parse(args).unwrap();
    if matches.opt_present("help") {
        println!("{}",
                 opts.usage(&format!("{} - find all unsafe blocks and print the \
                                      unsafe actions within them", name)));
        return;
    }

    let nonffi = matches.opt_present("nonffi");
    let ffi = matches.opt_present("ffi");
    let mut search_paths = SearchPaths::new();
    for path in matches.opt_strs("L").into_iter() {
        search_paths.add_path(&*path)
    }
    search_paths.add_path(DEFAULT_LIB_DIR);
    let externs = HashMap::new();

    let session = Arc::new(Session {
        nonffi: nonffi,
        ffi: ffi,
        externs: externs,
        search_paths: search_paths,
    });

    for name in matches.free.iter() {
        let sess = session.clone();
        let name = Path::new(name).to_owned();
        // the compiler has all sorts of thread locals.
        thread::spawn(move || {
            sess.run_library(name);
        }).join().unwrap();
    }
}


struct Session {
    nonffi: bool,
    ffi: bool,
    externs: Externs,
    search_paths: SearchPaths,
}

impl Session {
    fn run_library(&self, path: PathBuf) {
        get_ast(path, self.search_paths.clone(), self.externs.clone(), |tcx| {
            let cm = tcx.sess.codemap();

            let mut visitor = visitor::UnsafeVisitor::new(tcx);
            visitor.check_crate(tcx.map.krate());

            for (_, info) in visitor.unsafes.iter() {
                // compiler generated block, so we don't care.
                if info.compiler { continue }

                let n = info.raw_deref.len()
                    + info.static_mut.len()
                    + info.unsafe_call.len()
                    + info.asm.len()
                    + info.transmute.len()
                    + info.transmute_imm_to_mut.len()
                    + info.cast_raw_ptr_const_to_mut.len();

                let f = info.ffi.len();

                if (self.nonffi && n > 0) || (self.ffi && f > 0) {
                    use syntax::codemap::Pos;

                    let mut v = Vec::new();
                    if self.nonffi {
                        for vv in [&info.raw_deref, &info.static_mut,
                                   &info.unsafe_call, &info.asm,
                                   &info.transmute,
                                   &info.transmute_imm_to_mut,
                                   &info.cast_raw_ptr_const_to_mut].iter() {
                            v.extend(vv.iter().cloned())
                        }
                    }
                    if self.ffi {
                        v.extend(info.ffi.iter().cloned())
                    }

                    let lo = cm.lookup_char_pos_adj(info.span.lo);

                    // print the summary line
                    println!("{}:{}:{}: {} with {:?}",
                             lo.filename,
                             lo.line, lo.col.to_usize() + 1,
                             if info.is_fn {"fn"} else {"block"},
                             *info);

                    // and the individual unsafe actions within each block
                    // (in source order)
                    v.sort_by(|a, b| a.lo.to_usize().cmp(&b.lo.to_usize()));

                    let mut seen = HashSet::new();
                    for s in &v {
                        let lines = cm.span_to_lines(*s);
                        match &*lines.lines {
                            [line_info, ..] => {
                                let line_num = line_info.line_index;
                                let t = (line_num, lines.file.name.clone());
                                if !seen.contains(&t) {
                                    seen.insert(t);
                                    let line = lines.file.get_line(line_num).unwrap();
                                    println!("{}", line);
                                }
                            }
                            _ => { println!("no lines"); }
                        }
                    }
                }
            }
        })
    }
}

pub type Externs = HashMap<String, Vec<String>>;

/// Extract the expanded ast of a krate, along with the codemap which
/// connects source code locations to the actual code.
fn get_ast<F: Fn(&ty::ctxt)>(path: PathBuf,
                             search_paths: SearchPaths, externs: Externs,
                             f: F) {
    use syntax::diagnostic;

    // cargo culted from rustdoc :(
    let input = config::Input::File(path);

    let sessopts = config::Options {
        maybe_sysroot: Some(env::current_exe().unwrap().parent().unwrap().to_owned()),
        externs: externs,
        search_paths: search_paths,
        .. config::basic_options().clone()
    };

    let codemap = syntax::codemap::CodeMap::new();
    let diagnostic_handler =
        diagnostic::default_handler(diagnostic::Auto, None, true);
    let span_diagnostic_handler =
        diagnostic::mk_span_handler(diagnostic_handler, codemap);

    let sess = session::build_session_(sessopts, None, span_diagnostic_handler);

    let cfg = config::build_configuration(&sess);

    let mut controller = driver::CompileController::basic();
    controller.after_analysis = driver::PhaseController {
        stop: rustc_driver::Compilation::Stop,
        callback: Box::new(|state| f(state.tcx.unwrap()))
    };

    driver::compile_input(sess, cfg, &input, &None, &None, None, controller);
}
