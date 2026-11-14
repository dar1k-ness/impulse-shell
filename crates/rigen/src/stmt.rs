//! Statements and control flow.
use qml::ir::{AssignOp, Expr, Pat, Stmt};

use crate::scope::{Fx, Sink, THIS, Target, lexical_names, lit_str, pat_ids};

impl Fx<'_> {
    /// Statements of a block whose scope is already open: function declarations are
    /// created first (closing over the block's env), as `exec_block`.
    pub fn block_body(&mut self, body: &[Stmt]) {
        self.enter_tdz(body);
        self.hoist_funcs(body);
        for s in body {
            self.stmt(s);
        }
    }

    fn hoist_funcs(&mut self, body: &[Stmt]) {
        for s in body {
            if let Stmt::Func(f) = s
                && let Some(n) = &f.name
            {
                let c = self.closure(f);
                let w = self.write_name(n, &c);
                self.line(&w);
            }
        }
    }

    /// A `{}` block with its own scope.
    pub fn block(&mut self, body: &[Stmt]) {
        let mut names = Vec::new();
        lexical_names(body, &mut names);
        self.line("{");
        let saved = self.open(&names, false);
        self.block_body(body);
        self.close(saved);
        self.line("}");
    }

    fn cond(&mut self, e: &Expr) -> String {
        let v = self.expr(e);
        format!("({v}).truthy()")
    }

    pub fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(Expr::Assign(AssignOp::Assign, p, rhs))
                if !matches!(p.shown(), Pat::Member(_)) =>
            {
                let v = self.expr(rhs);
                self.bind(p, &v, false);
            }
            Stmt::Expr(e) => {
                let v = self.expr(e);
                self.line(&format!("let _ = {v};"));
            }
            Stmt::Var(kind, decls) => {
                for (p, init) in decls {
                    match init {
                        None if *kind == "var"
                            && matches!(p.shown(), Pat::Id(_) | Pat::Local(..)) => {}
                        None => self.bind(p, "Value::Undefined", true),
                        Some(e) => {
                            let v = self.expr(e);
                            self.bind(p, &v, true);
                        }
                    }
                }
            }
            Stmt::Func(_) | Stmt::Empty => {}
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.expr(e),
                    None => "Value::Undefined".into(),
                };
                if self.ctor && !self.derived_ctor {
                    // A constructor's object result replaces `this` (`construct`); others do not.
                    let t = self.tmp();
                    self.line(&format!("let {t} = {v}; let {t} = if matches!({t}, Value::Obj(_)) {{ {t} }} else {{ Value::Undefined }};"));
                    self.ret(&t);
                } else {
                    let t = self.tmp();
                    self.line(&format!("let {t} = {v};"));
                    self.ret(&t);
                }
            }
            Stmt::If(c, a, b) => {
                let c = self.cond(c);
                self.line(&format!("if {c} {{"));
                self.stmt(a);
                if let Some(b) = b {
                    self.line("} else {");
                    self.stmt(b);
                }
                self.line("}");
            }
            Stmt::Block(b) => self.block(b),
            Stmt::For(..)
            | Stmt::ForOf(..)
            | Stmt::ForIn(..)
            | Stmt::While(..)
            | Stmt::DoWhile(..) => self.looped(s, Vec::new()),
            Stmt::Labeled(l, body) => {
                // ECMA-262 §14.13.4: every label of a nested chain is in the label set.
                let mut labels = vec![l.clone()];
                let mut body = &**body;
                while let Stmt::Labeled(l, b) = body {
                    labels.push(l.clone());
                    body = b;
                }
                if matches!(
                    body,
                    Stmt::For(..)
                        | Stmt::ForOf(..)
                        | Stmt::ForIn(..)
                        | Stmt::While(..)
                        | Stmt::DoWhile(..)
                ) {
                    self.looped(body, labels);
                } else {
                    let b = self.g.id("b");
                    self.targets.push(Target::Block {
                        labels,
                        brk: b.clone(),
                    });
                    self.line(&format!("'{b}: {{"));
                    self.stmt(body);
                    self.line("}");
                    self.targets.pop();
                }
            }
            Stmt::Switch(d, cases) => self.switch(d, cases),
            Stmt::Break(l) => self.jump(false, l.as_deref()),
            Stmt::Continue(l) => self.jump(true, l.as_deref()),
            Stmt::Class(c) => {
                let v = self.class_value(c);
                if let Some(n) = &c.name {
                    let w = match self.find(n) {
                        Some(st) => self.write(&st, &v),
                        None => self.write_name(n, &v),
                    };
                    self.line(&w);
                    self.initialized(n);
                } else {
                    self.line(&format!("let _ = {v};"));
                }
            }
            Stmt::Throw(e) => {
                let v = self.expr(e);
                let t = self.tmp();
                let ec = self.err_code(&format!("jsrt::throw({t})"));
                self.line(&format!("{{ let {t} = {v}; {ec}; }}"));
            }
            Stmt::Try(body, catch, finally) => {
                self.try_stmt(body, catch.as_ref(), finally.as_deref())
            }
            // Compiled code has no object environments: `ir::unsupported` fails the build.
            Stmt::With(..) => {
                let r = self.t(&format!(
                    "cx.throw::<Value>(\"SyntaxError\", {})",
                    lit_str("unsupported statement with")
                ));
                self.line(&format!("let _ = {r};"));
            }
            Stmt::Unsupported(x) => {
                let r = self.t(&format!(
                    "cx.throw::<Value>(\"SyntaxError\", {})",
                    lit_str(&format!("unsupported statement {x}"))
                ));
                self.line(&format!("let _ = {r};"));
            }
        }
    }

    /// `return v` (`v` a Rust expression), through any enclosing `finally`.
    pub(crate) fn ret(&mut self, v: &str) {
        for t in self.targets.iter_mut().rev() {
            if let Target::Finally { lab, ret, .. } = t {
                *ret = true;
                let l = lab.clone();
                self.line(&format!("break '{l} Cpl::R({v});"));
                return;
            }
        }
        if self.derived_ctor {
            let t = self.read(&self.find(THIS).unwrap());
            self.line(&format!("return cx.derived_result({v}, {t});"));
        } else {
            self.line(&format!("return Ok({v});"));
        }
    }

    /// `break`/`continue` (optionally labeled), through any enclosing `finally`.
    fn jump(&mut self, cont: bool, label: Option<&str>) {
        let mut fin: Option<usize> = None;
        for i in (0..self.targets.len()).rev() {
            let code = match &self.targets[i] {
                Target::Finally { .. } => {
                    fin.get_or_insert(i);
                    continue;
                }
                Target::Loop {
                    labels,
                    brk,
                    cont: c,
                } => {
                    if label.is_none_or(|l| labels.iter().any(|x| x == l)) {
                        if cont {
                            c.clone()
                        } else {
                            format!("break '{brk}")
                        }
                    } else {
                        continue;
                    }
                }
                Target::Switch { brk } if !cont && label.is_none() => format!("break '{brk}"),
                Target::Block { labels, brk }
                    if !cont && label.is_some_and(|l| labels.iter().any(|x| x == l)) =>
                {
                    format!("break '{brk}")
                }
                _ => continue,
            };
            if let Some(fi) = fin {
                let Target::Finally { lab, jumps, .. } = &mut self.targets[fi] else {
                    unreachable!()
                };
                jumps.push((cont, label.map(str::to_string)));
                let id = jumps.len() - 1;
                let l = lab.clone();
                self.line(&format!("break '{l} Cpl::J({id});"));
            } else {
                self.line(&format!("{code};"));
            }
            return;
        }
        let r = self.t("cx.throw::<Value>(\"SyntaxError\", \"illegal break/continue\")");
        self.line(&format!("let _ = {r};"));
    }

    fn tick(&mut self) {
        let t = self.t("rt::tick(cx)");
        self.line(&format!("{t};"));
    }

    fn looped(&mut self, s: &Stmt, labels: Vec<String>) {
        let l = self.g.id("l");
        match s {
            Stmt::While(t, body) => {
                self.line(&format!("'{l}: loop {{"));
                let c = self.cond(t);
                self.line(&format!("if !{c} {{ break '{l}; }}"));
                self.tick();
                self.targets.push(Target::Loop {
                    labels,
                    brk: l.clone(),
                    cont: format!("continue '{l}"),
                });
                self.stmt(body);
                self.targets.pop();
                self.line("}");
            }
            Stmt::DoWhile(body, t) => {
                let b = self.g.id("b");
                self.line(&format!("'{l}: loop {{"));
                self.tick();
                self.targets.push(Target::Loop {
                    labels,
                    brk: l.clone(),
                    cont: format!("break '{b}"),
                });
                self.line(&format!("'{b}: {{"));
                self.stmt(body);
                self.line("}");
                self.targets.pop();
                let c = self.cond(t);
                self.line(&format!("if !{c} {{ break '{l}; }}"));
                self.line("}");
            }
            Stmt::For(init, test, update, body) => {
                let mut names = Vec::new();
                if let Some(i) = init {
                    lexical_names(std::slice::from_ref(&**i), &mut names);
                }
                self.line("{");
                let saved = self.open(&names, false);
                if let Some(i) = init {
                    self.stmt(i);
                }
                // Captured `let` bindings get a fresh Env per iteration (copy of the last one).
                let per_iter =
                    self.env != saved.0 && matches!(init.as_deref(), Some(Stmt::Var("let", _)));
                if per_iter {
                    let e = self.g.id("e");
                    self.line(&format!("let mut {e} = {}.iteration_copy();", self.env));
                    self.env = e;
                }
                let b = self.g.id("b");
                self.line(&format!("'{l}: loop {{"));
                self.tick();
                if let Some(t) = test {
                    let c = self.cond(t);
                    self.line(&format!("if !{c} {{ break '{l}; }}"));
                }
                self.targets.push(Target::Loop {
                    labels,
                    brk: l.clone(),
                    cont: format!("break '{b}"),
                });
                self.line(&format!("'{b}: {{"));
                self.stmt(body);
                self.line("}");
                self.targets.pop();
                if per_iter {
                    self.line(&format!("{0} = {0}.iteration_copy();", self.env));
                }
                if let Some(u) = update {
                    let v = self.expr(u);
                    self.line(&format!("let _ = {v};"));
                }
                self.line("}");
                self.close(saved);
                self.line("}");
            }
            Stmt::ForOf(k, p, e, body) => self.for_of(*k, p, e, body, labels, &l),
            Stmt::ForIn(k, p, e, body) => {
                let v = self.expr(e);
                let (tv, keys, it) = (self.tmp(), self.tmp(), self.tmp());
                let next = self.t(&format!("rt::for_in_next(cx, &mut {keys})"));
                self.line(&format!(
                    "{{ let {tv} = {v}; let mut {keys} = rt::for_in(cx, &{tv});"
                ));
                self.line(&format!("'{l}: while let Some({it}) = {next} {{"));
                self.tick();
                // `let`/`const` get a fresh binding per iteration; a `var` or a target is assigned.
                let lexical = k.is_some_and(|k| k != "var");
                let saved = lexical.then(|| {
                    let mut names = Vec::new();
                    pat_ids(p, &mut names);
                    self.open(&names, false)
                });
                self.bind(p, &it, lexical);
                self.targets.push(Target::Loop {
                    labels,
                    brk: l.clone(),
                    cont: format!("continue '{l}"),
                });
                self.stmt(body);
                self.targets.pop();
                if let Some(saved) = saved {
                    self.close(saved);
                }
                self.line("} }");
            }
            _ => unreachable!(),
        }
    }

    /// `for (p of e) body` (ECMA-262 ForIn/OfBodyEvaluation): steps the iterator, and a
    /// completion leaving the loop early closes it (IteratorClose) the way a `finally` runs.
    fn for_of(
        &mut self,
        k: Option<&str>,
        p: &Pat,
        e: &Expr,
        body: &Stmt,
        labels: Vec<String>,
        l: &str,
    ) {
        let v = self.expr(e);
        let (tv, src, it, c, r) = (self.tmp(), self.tmp(), self.tmp(), self.tmp(), self.tmp());
        let (fl, tl) = (self.g.id("f"), self.g.id("t"));
        let gi = self.t(&format!("cx.get_iterator(&{tv})"));
        self.line(&format!("{{ let {tv} = {v}; let mut {src} = {gi};"));
        self.targets.push(Target::Finally {
            lab: fl.clone(),
            jumps: Vec::new(),
            ret: false,
        });
        self.line(&format!("let {c}: Cpl = '{fl}: {{"));
        self.line(&format!("let {r}: Result<(), Ex> = '{tl}: {{"));
        let sink = std::mem::replace(&mut self.sink, Sink::Res(tl.clone()));
        let step = self.t(&format!("cx.iter_step(&mut {src})"));
        self.line(&format!("'{l}: while let Some({it}) = {step} {{"));
        self.tick();
        // `let`/`const` get a fresh binding per iteration; a `var` or a target is assigned.
        let lexical = k.is_some_and(|k| k != "var");
        let saved = lexical.then(|| {
            let mut names = Vec::new();
            pat_ids(p, &mut names);
            self.open(&names, false)
        });
        self.bind(p, &it, lexical);
        self.targets.push(Target::Loop {
            labels,
            brk: l.to_string(),
            cont: format!("continue '{l}"),
        });
        self.stmt(body);
        self.targets.pop();
        if let Some(saved) = saved {
            self.close(saved);
        }
        self.line("}");
        self.sink = sink;
        self.line("Ok(()) };");
        self.line(&format!(
            "match {r} {{ Ok(()) => Cpl::N, Err(e) => Cpl::T(e) }} }};"
        ));
        let Some(Target::Finally { jumps, .. }) = self.targets.pop() else {
            unreachable!()
        };
        // An exhausted iterator is done, which makes closing it a no-op.
        let close = self.t(&format!("cx.iter_close(&mut {src})"));
        let rv = self.tmp();
        self.line(&format!(
            "match {c} {{ Cpl::N => {{ {close}; }} Cpl::R({rv}) => {{ {close};"
        ));
        self.ret(&rv);
        self.line(&format!("}} Cpl::J(j) => {{ {close}; match j {{"));
        for (i, (cont, label)) in jumps.iter().enumerate() {
            self.line(&format!("{i} => {{"));
            self.jump(*cont, label.as_deref());
            self.line("}");
        }
        let rethrow = self.err_code("e");
        self.line(&format!(
            "_ => {{}} }} }} Cpl::T(e) => {{ let e = cx.iter_close_throw(&mut {src}, e); {rethrow}; }} }} }}"
        ));
    }

    fn switch(&mut self, d: &Expr, cases: &[(Option<Expr>, Vec<Stmt>)]) {
        let dv = self.expr(d);
        let (td, st) = (self.tmp(), self.tmp());
        self.line(&format!("{{ let {td} = {dv};"));
        let mut names = Vec::new();
        cases.iter().for_each(|(_, b)| lexical_names(b, &mut names));
        let saved = self.open(&names, false);
        if let Some(sc) = self.scopes.last_mut() {
            sc.switch = true;
        }
        cases.iter().for_each(|(_, b)| self.enter_tdz(b));
        // One block scope for all cases: every case's functions exist from the start.
        cases.iter().for_each(|(_, b)| self.hoist_funcs(b));
        self.line(&format!("let mut {st}: usize = usize::MAX;"));
        let m = self.g.id("m");
        self.line(&format!("'{m}: {{"));
        for (i, (t, _)) in cases.iter().enumerate() {
            if let Some(t) = t {
                let v = self.expr(t);
                self.line(&format!(
                    "if ({v}).strict_eq(&{td}) {{ {st} = {i}; break '{m}; }}"
                ));
            }
        }
        if let Some(di) = cases.iter().position(|(t, _)| t.is_none()) {
            self.line(&format!("{st} = {di};"));
        }
        self.line("}");
        let s = self.g.id("s");
        self.targets.push(Target::Switch { brk: s.clone() });
        self.line(&format!("'{s}: {{"));
        for (i, (_, body)) in cases.iter().enumerate() {
            self.line(&format!("if {st} <= {i} {{"));
            body.iter().for_each(|s| self.stmt(s));
            self.line("}");
        }
        self.line("}");
        self.targets.pop();
        self.close(saved);
        self.line("}");
    }

    fn catch_body(&mut self, p: Option<&Pat>, cb: &[Stmt], ev: &str) {
        let mut names = Vec::new();
        p.iter().for_each(|p| pat_ids(p, &mut names));
        lexical_names(cb, &mut names);
        let saved = self.open(&names, false);
        if let Some(p) = p {
            self.bind(p, ev, true);
        }
        self.block_body(cb);
        self.close(saved);
    }

    fn try_stmt(
        &mut self,
        body: &[Stmt],
        catch: Option<&(Option<Pat>, Vec<Stmt>)>,
        finally: Option<&[Stmt]>,
    ) {
        let tl = self.g.id("t");
        let r = self.tmp();
        let Some(fb) = finally else {
            self.line(&format!("let {r}: Result<(), Ex> = '{tl}: {{"));
            let sink = std::mem::replace(&mut self.sink, Sink::Res(tl.clone()));
            self.block(body);
            self.sink = sink;
            self.line("Ok(()) };");
            if let Some((p, cb)) = catch {
                let ev = self.tmp();
                let rethrow = self.err_code("e");
                self.line(&format!(
                    "if let Err(e) = {r} {{ match jsrt::caught(e) {{ Ok({ev}) => {{"
                ));
                self.catch_body(p.as_ref(), cb, &ev);
                self.line(&format!("}} Err(e) => {{ {rethrow}; }} }} }}"));
            } else {
                let rethrow = self.err_code("e");
                self.line(&format!("if let Err(e) = {r} {{ {rethrow}; }}"));
            }
            return;
        };
        let fl = self.g.id("f");
        let c = self.tmp();
        self.targets.push(Target::Finally {
            lab: fl.clone(),
            jumps: Vec::new(),
            ret: false,
        });
        self.line(&format!("let {c}: Cpl = '{fl}: {{"));
        self.line(&format!("let {r}: Result<(), Ex> = '{tl}: {{"));
        let sink = std::mem::replace(&mut self.sink, Sink::Res(tl.clone()));
        self.block(body);
        self.line("Ok(()) };");
        match catch {
            Some((p, cb)) => {
                let ev = self.tmp();
                self.line(&format!(
                    "match {r} {{ Ok(()) => Cpl::N, Err(e) => match jsrt::caught(e) {{ Ok({ev}) => {{"
                ));
                self.sink = Sink::Cpl(fl.clone());
                self.catch_body(p.as_ref(), cb, &ev);
                self.line("Cpl::N } Err(e) => Cpl::T(e) } } };");
            }
            None => self.line(&format!(
                "match {r} {{ Ok(()) => Cpl::N, Err(e) => Cpl::T(e) }} }};"
            )),
        }
        self.sink = sink;
        let Some(Target::Finally { jumps, .. }) = self.targets.pop() else {
            unreachable!()
        };
        self.block(fb);
        let v = self.tmp();
        self.line(&format!("match {c} {{ Cpl::N => {{}} Cpl::R({v}) => {{"));
        self.ret(&v);
        self.line("} Cpl::J(j) => match j {");
        for (i, (cont, label)) in jumps.iter().enumerate() {
            self.line(&format!("{i} => {{"));
            self.jump(*cont, label.as_deref());
            self.line("}");
        }
        let rethrow = self.err_code("e");
        self.line(&format!("_ => {{}} }}, Cpl::T(e) => {{ {rethrow}; }} }}"));
    }
}
