//! Translating AIG constraints into CNF: Tseitin's transformation with Plaisted–Greenbaum
//! polarity optimisation.
//!
//! Each AND gate `v = a ∧ b` that a constraint depends on gets a SAT variable `v`, defined by up
//! to two groups of clauses:
//!
//! * the **positive** direction `v → a ∧ b`, i.e. clauses `(¬v ∨ a)` and `(¬v ∨ b)`;
//! * the **negative** direction `a ∧ b → v`, i.e. the clause `(¬a ∨ ¬b ∨ v)`.
//!
//! A gate whose literal only ever has to *imply* its function needs only the positive direction,
//! and dually for the negative one (Plaisted–Greenbaum). The encoder records which directions it
//! has emitted for each gate and adds missing ones on demand. This keeps it correct when
//! later requests, such as blocking clauses or assumptions, need more than earlier ones did.

use std::collections::HashSet;

use redsat_sat::{ClauseSink, Lit, Var};

use crate::aig::{Aig, Edge, Node, NodeId};

/// The direction(s) in which a literal must faithfully represent its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polarity {
    /// The literal must imply the node's function.
    Positive,
    /// The node's function must imply the literal.
    Negative,
    /// The literal must be equivalent to the node's function.
    Both,
}

impl Polarity {
    fn negate_if(self, flip: bool) -> Self {
        match (self, flip) {
            (Self::Positive, true) => Self::Negative,
            (Self::Negative, true) => Self::Positive,
            (polarity, _) => polarity,
        }
    }

    fn needs_positive(self) -> bool {
        matches!(self, Self::Positive | Self::Both)
    }

    fn needs_negative(self) -> bool {
        matches!(self, Self::Negative | Self::Both)
    }
}

/// Which directions of a gate's definition have been emitted.
#[derive(Clone, Copy, Debug, Default)]
struct Emitted {
    positive: bool,
    negative: bool,
}

/// Incrementally translates parts of an [`Aig`] into CNF.
///
/// An encoder belongs to one AIG and one [`ClauseSink`] for their whole lifetime. It remembers
/// which SAT variable represents each node and which clauses it has already emitted, so repeated
/// requests only add what is missing.
#[derive(Debug, Default)]
pub struct CnfEncoder {
    vars: Vec<Option<Var>>,
    emitted: Vec<Emitted>,
    true_var: Option<Var>,
}

impl CnfEncoder {
    /// Creates an encoder that has not emitted anything yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The SAT variable representing `node`, if one has been allocated.
    #[must_use]
    pub fn var(&self, node: NodeId) -> Option<Var> {
        self.vars.get(node.index()).copied().flatten()
    }

    /// Adds clauses that force `edge` to be true.
    ///
    /// Top-level structure is exploited without auxiliary variables: a required conjunction is
    /// split into separately required conjuncts, and a required disjunction becomes one clause.
    pub fn require<S: ClauseSink + ?Sized>(&mut self, aig: &Aig, edge: Edge, sink: &mut S) {
        let mut pending = vec![edge];
        let mut seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !seen.insert(edge) {
                continue;
            }
            match aig.node(edge.node()) {
                Node::And(a, b) if !edge.is_complemented() => pending.extend([a, b]),
                _ => self.require_disjunction(aig, edge, sink),
            }
        }
    }

    /// Returns a literal that is *equivalent* to `edge` in every model, emitting whatever
    /// clauses that requires. Use it for assumptions and for reading circuit values from a model.
    pub fn literal<S: ClauseSink + ?Sized>(&mut self, aig: &Aig, edge: Edge, sink: &mut S) -> Lit {
        self.literal_with(aig, edge, Polarity::Both, sink)
    }

    /// Requires `edge`, a disjunction (a complemented AND) or a single leaf, as one clause.
    fn require_disjunction<S: ClauseSink + ?Sized>(&mut self, aig: &Aig, edge: Edge, sink: &mut S) {
        // Flatten nested disjunctions, ¬(x ∧ y) = ¬x ∨ ¬y, down to their leaves.
        let mut leaves = Vec::new();
        let mut pending = vec![edge];
        let mut seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !seen.insert(edge) {
                continue;
            }
            match aig.node(edge.node()) {
                Node::And(a, b) if edge.is_complemented() => pending.extend([!a, !b]),
                _ => leaves.push(edge),
            }
        }

        let mut clause = Vec::with_capacity(leaves.len());
        for leaf in leaves {
            match leaf.as_constant() {
                Some(true) => return, // the clause is satisfied: nothing to require
                Some(false) => {}     // a false literal contributes nothing
                None => clause.push(self.literal_with(aig, leaf, Polarity::Positive, sink)),
            }
        }
        if clause.iter().any(|&lit| clause.contains(&!lit)) {
            return; // tautology: x ∨ ¬x ∨ …
        }
        clause.sort_unstable();
        clause.dedup();
        sink.add_clause(&clause);
    }

    fn literal_with<S: ClauseSink + ?Sized>(
        &mut self,
        aig: &Aig,
        edge: Edge,
        polarity: Polarity,
        sink: &mut S,
    ) -> Lit {
        if let Some(value) = edge.as_constant() {
            let true_lit = self.true_lit(sink);
            return if value { true_lit } else { !true_lit };
        }
        self.encode(
            aig,
            edge.node(),
            polarity.negate_if(edge.is_complemented()),
            sink,
        );
        self.node_lit(edge, sink)
    }

    /// Emits the missing definition clauses for `root` and, transitively, its fan-in cone.
    /// It uses an explicit work list, because arithmetic circuits can be thousands of gates deep.
    fn encode<S: ClauseSink + ?Sized>(
        &mut self,
        aig: &Aig,
        root: NodeId,
        polarity: Polarity,
        sink: &mut S,
    ) {
        let mut pending = vec![(root, polarity)];
        while let Some((node, polarity)) = pending.pop() {
            let Node::And(a, b) = aig.node(node) else {
                continue; // constants and inputs need no defining clauses
            };
            let v = self.var_of(node, sink).pos();
            let (la, lb) = (self.node_lit(a, sink), self.node_lit(b, sink));
            let emitted = self.emitted[node.index()];

            if polarity.needs_positive() && !emitted.positive {
                self.emitted[node.index()].positive = true;
                sink.add_clause(&[!v, la]);
                sink.add_clause(&[!v, lb]);
                pending.push((a.node(), Polarity::Positive.negate_if(a.is_complemented())));
                pending.push((b.node(), Polarity::Positive.negate_if(b.is_complemented())));
            }
            if polarity.needs_negative() && !emitted.negative {
                self.emitted[node.index()].negative = true;
                sink.add_clause(&[!la, !lb, v]);
                pending.push((a.node(), Polarity::Negative.negate_if(a.is_complemented())));
                pending.push((b.node(), Polarity::Negative.negate_if(b.is_complemented())));
            }
        }
    }

    /// The literal of a non-constant edge, allocating its node's variable if needed.
    /// (Gate operands are never constant: [`Aig::and`] folds constants away.)
    fn node_lit<S: ClauseSink + ?Sized>(&mut self, edge: Edge, sink: &mut S) -> Lit {
        debug_assert!(
            edge.as_constant().is_none(),
            "constant edges have no node variable"
        );
        Lit::new(self.var_of(edge.node(), sink), edge.is_complemented())
    }

    fn var_of<S: ClauseSink + ?Sized>(&mut self, node: NodeId, sink: &mut S) -> Var {
        if node.index() >= self.vars.len() {
            self.vars.resize(node.index() + 1, None);
            self.emitted.resize(node.index() + 1, Emitted::default());
        }
        *self.vars[node.index()].get_or_insert_with(|| sink.new_var())
    }

    /// A literal fixed to true, used to represent constant edges as literals.
    fn true_lit<S: ClauseSink + ?Sized>(&mut self, sink: &mut S) -> Lit {
        let var = *self.true_var.get_or_insert_with(|| {
            let var = sink.new_var();
            sink.add_clause(&[var.pos()]);
            var
        });
        var.pos()
    }
}

#[cfg(test)]
mod tests {
    use redsat_sat::Cnf;

    use super::*;

    #[test]
    fn required_conjunctions_become_unit_clauses_without_gate_variables() {
        let mut aig = Aig::new();
        let inputs: Vec<Edge> = (0..4).map(|_| aig.new_input()).collect();
        let all = aig.and_all(inputs.iter().copied());

        let mut cnf = Cnf::new();
        CnfEncoder::new().require(&aig, all, &mut cnf);
        assert_eq!(cnf.num_vars(), 4, "only the inputs need variables");
        assert_eq!(cnf.num_clauses(), 4);
        assert!(cnf.clauses().iter().all(|clause| clause.len() == 1));
    }

    #[test]
    fn required_disjunctions_become_a_single_clause() {
        let mut aig = Aig::new();
        let inputs: Vec<Edge> = (0..4).map(|_| aig.new_input()).collect();
        let any = aig.or_all(inputs.iter().copied());

        let mut cnf = Cnf::new();
        CnfEncoder::new().require(&aig, any, &mut cnf);
        assert_eq!(cnf.num_vars(), 4);
        assert_eq!(cnf.clauses().len(), 1);
        assert_eq!(cnf.clauses()[0].len(), 4);
    }

    #[test]
    fn requiring_a_positive_gate_emits_only_its_positive_direction() {
        let mut aig = Aig::new();
        let (a, b, c) = (aig.new_input(), aig.new_input(), aig.new_input());
        let ab = aig.and(a, b);
        let clause = aig.or(ab, c); // (a ∧ b) ∨ c: the gate a ∧ b appears positively only

        let mut cnf = Cnf::new();
        CnfEncoder::new().require(&aig, clause, &mut cnf);
        // One clause (g ∨ c), plus g → a and g → b; no (¬a ∨ ¬b ∨ g).
        assert_eq!(cnf.num_vars(), 4);
        assert_eq!(cnf.num_clauses(), 3);
    }

    #[test]
    fn constants_require_nothing_or_the_empty_clause() {
        let aig = Aig::new();
        let mut cnf = Cnf::new();
        let mut encoder = CnfEncoder::new();
        encoder.require(&aig, Edge::TRUE, &mut cnf);
        assert_eq!(cnf.num_clauses(), 0);
        encoder.require(&aig, Edge::FALSE, &mut cnf);
        assert_eq!(cnf.clauses(), &[Vec::<Lit>::new()]);
    }

    #[test]
    fn tautological_clauses_are_dropped() {
        let mut aig = Aig::new();
        let (a, b) = (aig.new_input(), aig.new_input());
        let ab = aig.and(a, b);
        let tautology = !aig.and(!ab, ab); // folds to TRUE already
        assert_eq!(tautology, Edge::TRUE);

        let excluded_middle = aig.or(a, !a);
        assert_eq!(excluded_middle, Edge::TRUE);

        // A genuine tautology the AIG cannot see structurally: (a ∨ b) ∨ ¬a.
        let a_or_b = aig.or(a, b);
        let sneaky = aig.or(a_or_b, !a);
        let mut cnf = Cnf::new();
        CnfEncoder::new().require(&aig, sneaky, &mut cnf);
        assert_eq!(cnf.num_clauses(), 0);
    }
}
