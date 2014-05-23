use rustc::middle::ty;
use rustc::middle::typeck::MethodCall;

use syntax::{ast, ast_util, ast_map};
use syntax::codemap::Span;
use syntax::parse::token;
use syntax::visit;
use syntax::visit::Visitor;

use std::fmt;
use std::mem::replace;
use collections::treemap::TreeMap;

#[deriving(Eq)]
enum UnsafeContext {
    Safe,
    Unsafe(ast::NodeId),
}

fn type_is_unsafe_function(ty: ty::t) -> bool {
    match ty::get(ty).sty {
        ty::ty_bare_fn(ref f) => f.fn_style == ast::UnsafeFn,
        ty::ty_closure(ref f) => f.fn_style == ast::UnsafeFn,
        _ => false,
    }
}

pub struct NodeInfo {
    pub span: Span,
    pub is_fn: bool,
    pub compiler: bool,
    pub ffi: Vec<Span>,
    pub raw_deref: Vec<Span>,
    pub static_mut: Vec<Span>,
    pub unsafe_call: Vec<Span>,
    pub transmute: Vec<Span>,
    pub transmute_imm_to_mut: Vec<Span>,

    // these are only picked up with written in unsafe blocks, but *
    // as *mut is legal anywhere.
    pub cast_raw_ptr_imm_to_mut: Vec<Span>,
    pub asm: Vec<Span>,
}

impl NodeInfo {
    fn new(span: Span, is_fn: bool, compiler: bool) -> NodeInfo {
        NodeInfo {
            span: span,
            is_fn: is_fn,
            compiler: compiler,
            ffi: Vec::new(),
            raw_deref: Vec::new(),
            static_mut: Vec::new(),
            unsafe_call: Vec::new(),
            transmute: Vec::new(),
            transmute_imm_to_mut: Vec::new(),
            cast_raw_ptr_imm_to_mut: Vec::new(),
            asm: Vec::new()
        }
    }
}
impl fmt::Show for NodeInfo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut first = true;
        macro_rules! p ( ($fmt: tt, $name: ident) => {
                if !self.$name.is_empty() {
                    if !first {
                        try!(write!(fmt, ", "));
                    } else {
                        first = false
                    }
                    try!(write!(fmt, concat!("{} ", $fmt), self.$name.len()))
                }
            })
        p!("asm", asm);
        p!("deref", raw_deref);
        p!("ffi", ffi);
        p!("static mut", static_mut);
        p!("transmute", transmute);
        p!("transmute & to &mut", transmute_imm_to_mut);
        p!("cast * to *mut", cast_raw_ptr_imm_to_mut);
        p!("unsafe call", unsafe_call);
        // silence dead assign warning
        if first {}
        Ok(())
    }
}

pub struct UnsafeVisitor<'tcx> {
    tcx: &'tcx ty::ctxt,

    /// Whether we're in an unsafe context.
    node_info: Option<(ast::NodeId, NodeInfo)>,
    pub unsafes: TreeMap<ast::NodeId, NodeInfo>,
}

impl<'tcx> UnsafeVisitor<'tcx> {
    pub fn new(tcx: &'tcx ty::ctxt) -> UnsafeVisitor<'tcx> {
        UnsafeVisitor {
            tcx: tcx,
            node_info: None,
            unsafes: TreeMap::new(),
        }
    }

    pub fn check_crate(&mut self, krate: &ast::Crate) {
        visit::walk_crate(self, krate, ())
    }

    fn info<'a>(&'a mut self) -> &'a mut NodeInfo {
        self.node_info.get_mut_ref().mut1()
    }

    fn check_ptr_cast(&mut self, span: Span, from: &ast::Expr, to: &ast::Expr) -> bool {
        let from_ty = ty::expr_ty(self.tcx, from);
        let to_ty = ty::expr_ty(self.tcx, to);

        match (&ty::get(from_ty).sty, &ty::get(to_ty).sty) {
            (&ty::ty_rptr(_, ty::mt { mutbl: ast::MutImmutable, .. }),
             &ty::ty_rptr(_, ty::mt { mutbl: ast::MutMutable, .. })) => {
                self.info().transmute_imm_to_mut.push(span);
                true
            }

            (&ty::ty_ptr(ty::mt { mutbl: ast::MutImmutable, .. }),
             &ty::ty_ptr(ty::mt { mutbl: ast::MutMutable, .. })) => {
                self.info().cast_raw_ptr_imm_to_mut.push(span);
                true
            }

            _ => {
                false
            }
        }
    }
}

impl<'tcx> Visitor<()> for UnsafeVisitor<'tcx> {
    fn visit_fn(&mut self, fn_kind: &visit::FnKind, fn_decl: &ast::FnDecl,
                block: &ast::Block, span: Span, node_id: ast::NodeId, _:()) {
        let (is_item_fn, is_unsafe_fn) = match *fn_kind {
            visit::FkItemFn(_, _, fn_style, _) =>
                (true, fn_style == ast::UnsafeFn),
            visit::FkMethod(_, _, method) =>
                (true, method.fn_style == ast::UnsafeFn),
            _ => (false, false),
        };

        let old_node_info = if is_unsafe_fn {
            replace(&mut self.node_info, Some((node_id, NodeInfo::new(span, true, false))))
        } else if is_item_fn {
            replace(&mut self.node_info, None)
        } else {
            None
        };
        visit::walk_fn(self, fn_kind, fn_decl, block, span, ());

        match replace(&mut self.node_info, old_node_info) {
            Some((id, info)) => assert!(self.unsafes.insert(id, info)),
            //Some((id, info)) => { self.unsafes.insert(id, info); }
            None => {}
        }
    }

    fn visit_block(&mut self, block: &ast::Block, _:()) {
        let (old_node_info, inserted) = match block.rules {
            ast::DefaultBlock => (None, false),
            ast::UnsafeBlock(source) => {
                let compiler = source == ast::CompilerGenerated;
                if self.node_info.is_none() || compiler {
                    (replace(&mut self.node_info,
                             Some((block.id, NodeInfo::new(block.span, false, compiler)))),
                     true)
                } else {
                    (None, false)
                }
            }
        };
        visit::walk_block(self, block, ());

        if inserted {
            match replace(&mut self.node_info, old_node_info) {
                Some((id, info)) => assert!(self.unsafes.insert(id, info)),
                //Some((id, info)) => { self.unsafes.insert(id, info); }
                None => {}
            }
        }
    }

    fn visit_expr(&mut self, expr: &ast::Expr, _:()) {
        if self.node_info.is_some() {
            match expr.node {
                ast::ExprMethodCall(_, _, _) => {
                    let method_call = MethodCall::expr(expr.id);
                    let base_type = self.tcx.method_map.borrow().get(&method_call).ty;
                    if type_is_unsafe_function(base_type) {
                        self.info().unsafe_call.push(expr.span)
                    }
                }
                ast::ExprCall(base, ref args) => {
                    match (&base.node, args.as_slice()) {
                        (&ast::ExprPath(ref p), [ref arg])
                            // ew, but whatever.
                            if p.segments.last().unwrap().identifier.name ==
                            token::intern("transmute") => {
                                if !self.check_ptr_cast(expr.span, *arg, expr) {
                                    // not a */& -> *mut/&mut cast.
                                    self.info().transmute.push(expr.span)
                                }
                            }

                        _ => {
                            match self.tcx.def_map.borrow().find(&base.id) {
                                Some(&ast::DefFn(did, ast::UnsafeFn)) => {
                                    if ast_util::is_local(did) {
                                        match self.tcx.map.get(did.node) {
                                            ast_map::NodeForeignItem(_) => {
                                                self.info().ffi.push(expr.span)
                                            }
                                            _ => self.info().unsafe_call.push(expr.span)
                                        }
                                    } else {
                                        // cross-crate calls are always just
                                        // unsafe calls.
                                        self.info().unsafe_call.push(expr.span)
                                    }
                                }
                                _ => {
                                    let base_type = ty::node_id_to_type(self.tcx, base.id);
                                    if type_is_unsafe_function(base_type) {
                                        self.info().unsafe_call.push(expr.span)
                                    }
                                }
                            }
                        }
                    }
                }

                ast::ExprUnary(ast::UnDeref, base) => {
                    let base_type = ty::node_id_to_type(self.tcx, base.id);
                    match ty::get(base_type).sty {
                        ty::ty_ptr(_) => {
                            self.info().raw_deref.push(expr.span)
                        }
                        _ => {}
                    }
                }
                ast::ExprInlineAsm(..) => {
                    self.info().asm.push(expr.span)
                }
                ast::ExprPath(..) => {
                    match ty::resolve_expr(self.tcx, expr) {
                        ast::DefStatic(_, true) => {
                            self.info().static_mut.push(expr.span)
                        }
                        _ => {}
                    }
                }
                ast::ExprCast(from, _) => {
                    self.check_ptr_cast(expr.span, from, expr);
                }
                _ => {}
            }
        }
        visit::walk_expr(self, expr, ());
    }
}
