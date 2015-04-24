use rustc::middle::{ty, def};
use rustc::middle::ty::MethodCall;

use syntax::{ast, ast_util, ast_map};
use syntax::codemap::Span;
use syntax::parse::token;
use syntax::visit;
use syntax::visit::Visitor;

use std::fmt;
use std::mem::replace;
use std::collections::BTreeMap;

fn type_is_unsafe_function(ty: ty::Ty) -> bool {
    match ty.sty {
        ty::ty_bare_fn(_, ref f) => f.unsafety == ast::Unsafety::Unsafe,
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

    // these are only picked up with written in unsafe blocks, but *const
    // as *mut is legal anywhere.
    pub cast_raw_ptr_const_to_mut: Vec<Span>,
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
            cast_raw_ptr_const_to_mut: Vec::new(),
            asm: Vec::new()
        }
    }
}
impl fmt::Debug for NodeInfo {
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
            });
        p!("asm", asm);
        p!("deref", raw_deref);
        p!("ffi", ffi);
        p!("static mut", static_mut);
        p!("transmute", transmute);
        p!("transmute & to &mut", transmute_imm_to_mut);
        p!("cast *const to *mut", cast_raw_ptr_const_to_mut);
        p!("unsafe call", unsafe_call);
        // silence dead assign warning
        if first {}
        Ok(())
    }
}

pub struct UnsafeVisitor<'tcx, 'a: 'tcx> {
    tcx: &'tcx ty::ctxt<'a>,

    /// Whether we're in an unsafe context.
    node_info: Option<(ast::NodeId, NodeInfo)>,
    pub unsafes: BTreeMap<ast::NodeId, NodeInfo>,
}

impl<'tcx, 'a> UnsafeVisitor<'tcx, 'a> {
    pub fn new(tcx: &'tcx ty::ctxt<'a>) -> UnsafeVisitor<'tcx, 'a> {
        UnsafeVisitor {
            tcx: tcx,
            node_info: None,
            unsafes: BTreeMap::new(),
        }
    }

    pub fn check_crate(&mut self, krate: &ast::Crate) {
        visit::walk_crate(self, krate)
    }

    fn info<'b>(&'b mut self) -> &'b mut NodeInfo {
        &mut self.node_info.as_mut().unwrap().1
    }

    fn check_ptr_cast(&mut self, span: Span, from: &ast::Expr, to: &ast::Expr) -> bool {
        let from_ty = ty::expr_ty(self.tcx, from);
        let to_ty = ty::expr_ty(self.tcx, to);

        match (&from_ty.sty, &to_ty.sty) {
            (&ty::ty_rptr(_, ty::mt { mutbl: ast::MutImmutable, .. }),
             &ty::ty_rptr(_, ty::mt { mutbl: ast::MutMutable, .. })) => {
                self.info().transmute_imm_to_mut.push(span);
                true
            }

            (&ty::ty_ptr(ty::mt { mutbl: ast::MutImmutable, .. }),
             &ty::ty_ptr(ty::mt { mutbl: ast::MutMutable, .. })) => {
                self.info().cast_raw_ptr_const_to_mut.push(span);
                true
            }

            _ => {
                false
            }
        }
    }
}

impl<'tcx,'a,'b> Visitor<'a> for UnsafeVisitor<'tcx,'b> {
    fn visit_fn(&mut self, fn_kind: visit::FnKind<'a>, fn_decl: &'a ast::FnDecl,
                block: &ast::Block, span: Span, node_id: ast::NodeId) {
        let (is_item_fn, is_unsafe_fn) = match fn_kind {
            visit::FkItemFn(_, _, fn_style, _, _) =>
                (true, fn_style == ast::Unsafety::Unsafe),
            visit::FkMethod(_, sig, _) =>
                (true, sig.unsafety == ast::Unsafety::Unsafe),
            _ => (false, false),
        };

        let old_node_info = if is_unsafe_fn {
            replace(&mut self.node_info, Some((node_id, NodeInfo::new(span, true, false))))
        } else if is_item_fn {
            replace(&mut self.node_info, None)
        } else {
            None
        };
        visit::walk_fn(self, fn_kind, fn_decl, block, span);

        match replace(&mut self.node_info, old_node_info) {
            Some((id, info)) => assert!(self.unsafes.insert(id, info).is_none()),
            //Some((id, info)) => { self.unsafes.insert(id, info); }
            None => {}
        }
    }

    fn visit_block(&mut self, block: &'a ast::Block) {
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
        visit::walk_block(self, block);

        if inserted {
            match replace(&mut self.node_info, old_node_info) {
                Some((id, info)) => assert!(self.unsafes.insert(id, info).is_none()),
                //Some((id, info)) => { self.unsafes.insert(id, info); }
                None => {}
            }
        }
    }

    fn visit_expr(&mut self, expr: &'a ast::Expr) {
        if self.node_info.is_some() {
            match expr.node {
                ast::ExprMethodCall(_, _, _) => {
                    let method_call = MethodCall::expr(expr.id);
                    let base_type = self.tcx.method_map.borrow()[&method_call].ty;
                    if type_is_unsafe_function(base_type) {
                        self.info().unsafe_call.push(expr.span)
                    }
                }
                ast::ExprCall(ref base, ref args) => {
                    match (&base.node, &**args) {
                        (&ast::ExprPath(_, ref p), [ref arg])
                            // ew, but whatever.
                            if p.segments.last().unwrap().identifier.name ==
                            token::intern("transmute") => {
                                if !self.check_ptr_cast(expr.span, &**arg, expr) {
                                    // not a */& -> *mut/&mut cast.
                                    self.info().transmute.push(expr.span)
                                }
                            }

                        _ => {
                            let is_ffi = match self.tcx.def_map.borrow().get(&base.id) {
                                Some(&def::PathResolution { base_def: def::DefFn(did, _), .. }) => {
                                    // cross-crate calls are always
                                    // just unsafe calls.
                                    ast_util::is_local(did) &&
                                        match self.tcx.map.get(did.node) {
                                            ast_map::NodeForeignItem(_) => true,
                                            _ => false
                                        }
                                }
                                _ => false
                            };

                            if is_ffi {
                                self.info().ffi.push(expr.span)
                            } else {
                                let base_type = ty::node_id_to_type(self.tcx, base.id);
                                if type_is_unsafe_function(base_type) {
                                    self.info().unsafe_call.push(expr.span)
                                }
                            }
                        }
                    }
                }

                ast::ExprUnary(ast::UnDeref, ref base) => {
                    let base_type = ty::node_id_to_type(self.tcx, base.id);
                    match base_type.sty {
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
                        def::DefStatic(_, true) => {
                            self.info().static_mut.push(expr.span)
                        }
                        _ => {}
                    }
                }
                ast::ExprCast(ref from, _) => {
                    self.check_ptr_cast(expr.span, &**from, expr);
                }
                _ => {}
            }
        }
        visit::walk_expr(self, expr);
    }
}
