#![crate_id="unsafe_ls"]
#![feature(managed_boxes, macro_rules)]

extern crate collections;
extern crate getopts;
extern crate syntax;
extern crate rustc;

use rustc::driver::{driver, session};
use rustc::middle::ty;
use rustc::middle::typeck::MethodMap;
use syntax::ast;
use std::os;
use collections::HashSet;

mod visitor;

static DEFAULT_LIB_DIR: &'static str = "/usr/local/lib/rustlib/x86_64-unknown-linux-gnu/lib";

fn main() {
    let args = std::os::args();
    let opts = ~[getopts::optflag("h", "help", "show this help message"),
                 getopts::optflag("n", "nonffi",
                                  "print `unsafe`s that include non-FFI unsafe behaviours"),
                 getopts::optflag("f", "ffi", "print `unsafe`s that do FFI calls"),
                 getopts::optmulti("L", "library-path",
                                   "directories to add to crate search path", "DIR")];

    let matches = getopts::getopts(args.tail(), opts).unwrap();
    if matches.opts_present([~"h", ~"help"]) {
        println!("{}",
                 getopts::usage(args[0] + " - find all unsafe blocks and print the \
                                unsafe actions within them",
                                opts));
        return;
    }

    let nonffi = matches.opts_present([~"n", ~"nonffi"]);
    let ffi = matches.opts_present([~"f", ~"ffi"]);
    let mut libs: HashSet<Path> = matches.opt_strs("L").move_iter().map(|s| Path::new(s)).collect();
    libs.insert(Path::new(DEFAULT_LIB_DIR));

    for name in matches.free.iter() {
        let (krate, tcx, method_map) = get_ast(Path::new(name.as_slice()), libs.clone());
        let cm = tcx.sess.codemap();

        let mut visitor = visitor::UnsafeVisitor::new(&tcx, method_map);
        visitor.check_crate(&krate);

        for (_, info) in visitor.unsafes.iter() {
            // compiler generated block, so we don't care.
            if info.compiler { continue }

            let n = info.raw_deref.len()
                + info.static_mut.len()
                + info.unsafe_call.len()
                + info.asm.len()
                + info.transmute.len();

            let f = info.ffi.len();

            if (nonffi && n > 0) || (ffi && f > 0) {
                use syntax::codemap::Pos;

                let mut v = Vec::new();
                if nonffi {
                    for vv in [&info.raw_deref, &info.static_mut,
                               &info.unsafe_call, &info.asm,
                               &info.transmute].iter() {
                        for s in vv.as_slice().iter() {
                            v.push(*s)
                        }
                    }
                }
                if ffi {
                    for s in info.ffi.as_slice().iter() {
                        v.push(*s)
                    }
                }

                let lo = cm.lookup_char_pos_adj(info.span.lo);

                // print the summary line
                println!("{}:{}:{}: {} with {}",
                         lo.filename,
                         lo.line, lo.col.to_uint() + 1,
                         if info.is_fn {"fn"} else {"block"},
                         *info);

                // and the the individual unsafe actions within each
                // block (in sorted order)
                v.as_mut_slice().sort_by(|a, b| a.lo.to_uint().cmp(&b.lo.to_uint()));

                let mut seen = HashSet::new();
                for s in v.as_slice().iter() {
                    let lines = cm.span_to_lines(*s);
                    match lines.lines.as_slice() {
                        [line_num, ..] => {
                            let t = (line_num, lines.file.name.clone());
                            if !seen.contains(&t) {
                                seen.insert(t);
                                let line = lines.file.get_line(line_num as int);
                                println!("{}", line);
                            }
                        }
                        _ => { println!("no lines"); }
                    }
                }
            }
        }
    }
}

/// Extract the expanded ast of a krate, along with the codemap which
/// connects source code locations to the actual code.
fn get_ast(path: Path, libs: HashSet<Path>) -> (ast::Crate, ty::ctxt, MethodMap) {
    use syntax::diagnostic;

    // cargo culted from rustdoc_ng :(
    let input = driver::FileInput(path);

    let sessopts = session::Options {
        maybe_sysroot: Some(os::self_exe_path().unwrap().dir_path()),
        addl_lib_search_paths: std::cell::RefCell::new(libs),
        .. session::basic_options().clone()
    };

    let codemap = syntax::codemap::CodeMap::new();
    let diagnostic_handler = diagnostic::default_handler();
    let span_diagnostic_handler =
        diagnostic::mk_span_handler(diagnostic_handler, codemap);

    let sess = driver::build_session_(sessopts, None, span_diagnostic_handler);

    let cfg = driver::build_configuration(&sess);

    let krate = driver::phase_1_parse_input(&sess, cfg, &input);
    let (krate, ast_map) = {
        let loader = &mut rustc::metadata::creader::Loader::new(&sess);

        driver::phase_2_configure_and_expand(&sess, loader, krate,
                                             &from_str("unsafe_ls").unwrap())
    };
    let res = driver::phase_3_run_analysis_passes(sess, &krate, ast_map);
    let driver::CrateAnalysis { ty_cx, maps, .. } = res;
    (krate, ty_cx, maps.method_map)
}
