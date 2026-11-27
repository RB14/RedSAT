//! And-Inverter Graphs: the Boolean circuit representation RedSAT bit-blasts programs into.
//!
//! An AIG expresses every Boolean function with two-input AND gates and complemented edges
//! (inverters are free). The graph is *hash-consed*: asking twice for the same gate returns the
//! same node. It is also *locally simplified*, so constants propagate through a circuit as it is
//! built.

use std::collections::HashMap;
use std::ops::Not;

/// The index of a node in an [`Aig`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// The position of this node in the graph's node list.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A reference to a node, possibly complemented.
///
/// Packed as `2 * node + complemented`. Node 0 is the constant `false`, so [`Edge::FALSE`] and
/// [`Edge::TRUE`] are the two edges into it, and they order before every other edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge(u32);

impl Edge {
    /// The constant `false`.
    pub const FALSE: Edge = Edge(0);
    /// The constant `true`.
    pub const TRUE: Edge = Edge(1);

    const fn new(node: NodeId, complemented: bool) -> Self {
        Self((node.0 << 1) | complemented as u32)
    }

    /// The edge for a Boolean constant.
    #[must_use]
    pub const fn constant(value: bool) -> Self {
        if value { Self::TRUE } else { Self::FALSE }
    }

    /// The node this edge points to.
    #[must_use]
    pub const fn node(self) -> NodeId {
        NodeId(self.0 >> 1)
    }

    /// Whether the edge inverts its node.
    #[must_use]
    pub const fn is_complemented(self) -> bool {
        self.0 & 1 == 1
    }

    /// The constant value of this edge, if it is [`Edge::TRUE`] or [`Edge::FALSE`].
    #[must_use]
    pub const fn as_constant(self) -> Option<bool> {
        match self.0 {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// This edge, complemented if `flip` is true.
    #[must_use]
    pub const fn negate_if(self, flip: bool) -> Self {
        Self(self.0 ^ flip as u32)
    }
}

impl Not for Edge {
    type Output = Edge;

    fn not(self) -> Edge {
        Edge(self.0 ^ 1)
    }
}

/// A node of an [`Aig`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    /// The constant `false`. It is always node 0.
    False,
    /// A free input. The payload is its position among the graph's inputs.
    Input(u32),
    /// The conjunction of two edges.
    And(Edge, Edge),
}

/// A hash-consed And-Inverter Graph.
///
/// Nodes are append-only and every AND node is created after its operands. The node order is
/// therefore a topological order, which evaluation relies on.
#[derive(Clone, Debug)]
pub struct Aig {
    nodes: Vec<Node>,
    inputs: Vec<NodeId>,
    strash: HashMap<(Edge, Edge), NodeId>,
}

impl Default for Aig {
    fn default() -> Self {
        Self::new()
    }
}

impl Aig {
    /// Creates a graph that contains only the constant node.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::False],
            inputs: Vec::new(),
            strash: HashMap::new(),
        }
    }

    /// The node with the given id.
    #[must_use]
    pub fn node(&self, id: NodeId) -> Node {
        self.nodes[id.index()]
    }

    /// The total number of nodes, including the constant and the inputs.
    #[must_use]
    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// The number of AND gates.
    #[must_use]
    pub fn num_ands(&self) -> usize {
        self.nodes.len() - 1 - self.inputs.len()
    }

    /// The input nodes, in creation order.
    #[must_use]
    pub fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    /// Creates a fresh, unconstrained input.
    pub fn new_input(&mut self) -> Edge {
        let position = u32::try_from(self.inputs.len()).expect("too many inputs");
        let id = self.push(Node::Input(position));
        self.inputs.push(id);
        Edge::new(id, false)
    }

    /// `a ∧ b`. This is the only operation that creates gates; every other one is derived from it.
    pub fn and(&mut self, a: Edge, b: Edge) -> Edge {
        // Normalise the operand order so that `a ∧ b` and `b ∧ a` share one node. Constants
        // sort first, which keeps the simplification rules below short.
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == Edge::FALSE || a == !b {
            return Edge::FALSE;
        }
        if a == Edge::TRUE || a == b {
            return b;
        }
        if let Some(simpler) = self.two_level(a, b) {
            return simpler;
        }
        if let Some(&id) = self.strash.get(&(a, b)) {
            return Edge::new(id, false);
        }
        let id = self.push(Node::And(a, b));
        self.strash.insert((a, b), id);
        Edge::new(id, false)
    }

    /// The operands of the AND node `edge` points to (ignoring its complement), if any.
    fn operands(&self, edge: Edge) -> Option<(Edge, Edge)> {
        match self.nodes[edge.node().index()] {
            Node::And(x, y) => Some((x, y)),
            _ => None,
        }
    }

    /// Two-level rewriting of `a ∧ b` when an operand is itself a conjunction, possibly negated
    /// (Brummayer and Biere, "Local Two-Level And-Inverter Graph Minimization without Blowup",
    /// 2006). None of the rules adds nodes: they find contradictions, repeated or subsumed
    /// operands, and resolvents that the structural hash alone would miss.
    fn two_level(&mut self, a: Edge, b: Edge) -> Option<Edge> {
        for (x, y) in [(a, b), (b, a)] {
            let Some((x0, x1)) = self.operands(x) else {
                continue;
            };
            let inner = self.operands(y);
            if x.is_complemented() {
                // x = ¬(x0 ∧ x1)
                if y == !x0 || y == !x1 {
                    return Some(y); // subsumption: ¬(x0 ∧ x1) ∧ ¬x0 = ¬x0
                }
                if y == x0 {
                    return Some(self.and(y, !x1)); // substitution: ¬(x0 ∧ x1) ∧ x0 = x0 ∧ ¬x1
                }
                if y == x1 {
                    return Some(self.and(y, !x0));
                }
                match inner {
                    Some((y0, y1)) if !y.is_complemented() => {
                        // y = y0 ∧ y1: subsumption (a child of x is refuted by y) …
                        if [y0, y1].iter().any(|&yj| yj == !x0 || yj == !x1) {
                            return Some(y);
                        }
                        // … and substitution (a child of x is implied by y).
                        if y0 == x0 || y1 == x0 {
                            return Some(self.and(y, !x1));
                        }
                        if y0 == x1 || y1 == x1 {
                            return Some(self.and(y, !x0));
                        }
                    }
                    Some((y0, y1)) => {
                        // resolution: ¬(p ∧ q) ∧ ¬(p ∧ ¬q) = ¬p
                        for (p, q) in [(x0, x1), (x1, x0)] {
                            if (y0 == p && y1 == !q) || (y1 == p && y0 == !q) {
                                return Some(!p);
                            }
                        }
                    }
                    None => {}
                }
            } else {
                // x = x0 ∧ x1
                if y == !x0 || y == !x1 {
                    return Some(Edge::FALSE); // contradiction
                }
                if y == x0 || y == x1 {
                    return Some(x); // idempotence
                }
                if let Some((y0, y1)) = inner {
                    if !y.is_complemented() && [y0, y1].iter().any(|&yj| yj == !x0 || yj == !x1) {
                        return Some(Edge::FALSE); // contradiction across both conjunctions
                    }
                }
            }
        }
        None
    }

    /// `a ∨ b`.
    pub fn or(&mut self, a: Edge, b: Edge) -> Edge {
        !self.and(!a, !b)
    }

    /// `a ⊕ b`, as `¬(a ∧ b) ∧ ¬(¬a ∧ ¬b)`.
    pub fn xor(&mut self, a: Edge, b: Edge) -> Edge {
        let both = self.and(a, b);
        let neither = self.and(!a, !b);
        self.and(!both, !neither)
    }

    /// `a ↔ b`.
    pub fn iff(&mut self, a: Edge, b: Edge) -> Edge {
        !self.xor(a, b)
    }

    /// `a → b`.
    pub fn implies(&mut self, a: Edge, b: Edge) -> Edge {
        self.or(!a, b)
    }

    /// If-then-else: `cond ? then : otherwise`.
    pub fn ite(&mut self, cond: Edge, then: Edge, otherwise: Edge) -> Edge {
        if then == otherwise {
            return then;
        }
        let when_true = self.and(cond, then);
        let when_false = self.and(!cond, otherwise);
        self.or(when_true, when_false)
    }

    /// The conjunction of all `edges` (`true` if there are none).
    pub fn and_all(&mut self, edges: impl IntoIterator<Item = Edge>) -> Edge {
        edges
            .into_iter()
            .fold(Edge::TRUE, |acc, edge| self.and(acc, edge))
    }

    /// The disjunction of all `edges` (`false` if there are none).
    pub fn or_all(&mut self, edges: impl IntoIterator<Item = Edge>) -> Edge {
        edges
            .into_iter()
            .fold(Edge::FALSE, |acc, edge| self.or(acc, edge))
    }

    /// Evaluates every node, given the value of each input in creation order.
    ///
    /// # Panics
    ///
    /// If `input_values` does not have exactly one value per input.
    #[must_use]
    pub fn simulate(&self, input_values: &[bool]) -> Simulation {
        assert_eq!(
            input_values.len(),
            self.inputs.len(),
            "need exactly one value per input"
        );
        let mut values = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let value = match *node {
                Node::False => false,
                Node::Input(position) => input_values[position as usize],
                Node::And(a, b) => {
                    let value_of =
                        |edge: Edge| values[edge.node().index()] != edge.is_complemented();
                    value_of(a) && value_of(b)
                }
            };
            values.push(value);
        }
        Simulation { values }
    }

    fn push(&mut self, node: Node) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("AIG too large"));
        self.nodes.push(node);
        id
    }
}

/// The value of every node of an [`Aig`] under one input assignment.
#[derive(Clone, Debug)]
pub struct Simulation {
    values: Vec<bool>,
}

impl Simulation {
    /// The value of `edge`.
    ///
    /// # Panics
    ///
    /// If `edge` refers to a node created after the simulation was run.
    #[must_use]
    pub fn value(&self, edge: Edge) -> bool {
        self.values[edge.node().index()] != edge.is_complemented()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A formula over four inputs, evaluated directly, to check the AIG's rewriting against.
    #[derive(Clone, Debug)]
    enum Formula {
        Input(usize),
        Not(Box<Formula>),
        And(Box<Formula>, Box<Formula>),
    }

    impl Formula {
        fn eval(&self, inputs: &[bool]) -> bool {
            match self {
                Self::Input(i) => inputs[*i],
                Self::Not(f) => !f.eval(inputs),
                Self::And(f, g) => f.eval(inputs) && g.eval(inputs),
            }
        }

        fn build(&self, aig: &mut Aig, inputs: &[Edge]) -> Edge {
            match self {
                Self::Input(i) => inputs[*i],
                Self::Not(f) => !f.build(aig, inputs),
                Self::And(f, g) => {
                    let (x, y) = (f.build(aig, inputs), g.build(aig, inputs));
                    aig.and(x, y)
                }
            }
        }
    }

    /// Random formulas (fixed seed), deep enough for the two-level rules to fire often.
    fn random_formula(state: &mut u64, depth: u32) -> Formula {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        let choice = *state % 7;
        if depth == 0 || choice < 2 {
            #[allow(clippy::cast_possible_truncation)]
            return Formula::Input((*state >> 8) as usize % 4);
        }
        if choice < 3 {
            return Formula::Not(Box::new(random_formula(state, depth - 1)));
        }
        Formula::And(
            Box::new(random_formula(state, depth - 1)),
            Box::new(random_formula(state, depth - 1)),
        )
    }

    #[test]
    fn rewriting_preserves_every_function() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for _ in 0..20_000 {
            let formula = random_formula(&mut state, 5);
            let mut aig = Aig::new();
            let inputs: Vec<Edge> = (0..4).map(|_| aig.new_input()).collect();
            let edge = formula.build(&mut aig, &inputs);
            for mask in 0..16_u32 {
                let values: Vec<bool> = (0..4).map(|i| mask >> i & 1 == 1).collect();
                assert_eq!(
                    aig.simulate(&values).value(edge),
                    formula.eval(&values),
                    "{formula:?} at {mask:04b}"
                );
            }
        }
    }

    #[test]
    fn two_level_rules_shrink_the_graph() {
        let mut aig = Aig::new();
        let (x, y, z) = (aig.new_input(), aig.new_input(), aig.new_input());
        let xy = aig.and(x, y);
        assert_eq!(aig.and(xy, !x), Edge::FALSE, "contradiction");
        assert_eq!(aig.and(xy, x), xy, "idempotence");
        assert_eq!(aig.and(!xy, !x), !x, "subsumption");
        let x_not_y = aig.and(x, !y);
        assert_eq!(aig.and(!xy, x), x_not_y, "substitution");
        let xz = aig.and(x, !y);
        assert_eq!(aig.and(!xy, !xz), !x, "resolution");
        let _ = z;
    }

    #[test]
    fn constants_fold_away() {
        let mut aig = Aig::new();
        let x = aig.new_input();
        assert_eq!(aig.and(x, Edge::FALSE), Edge::FALSE);
        assert_eq!(aig.and(Edge::TRUE, x), x);
        assert_eq!(aig.and(x, x), x);
        assert_eq!(aig.and(x, !x), Edge::FALSE);
        assert_eq!(aig.or(x, !x), Edge::TRUE);
        assert_eq!(aig.xor(x, x), Edge::FALSE);
        assert_eq!(aig.xor(x, Edge::TRUE), !x);
        assert_eq!(aig.ite(Edge::TRUE, x, Edge::FALSE), x);
        assert_eq!(aig.num_ands(), 0, "no gate was ever needed");
    }

    #[test]
    fn gates_are_hash_consed() {
        let mut aig = Aig::new();
        let (x, y) = (aig.new_input(), aig.new_input());
        let first = aig.and(x, !y);
        let second = aig.and(!y, x);
        assert_eq!(first, second);
        assert_eq!(aig.num_ands(), 1);
    }

    #[test]
    fn derived_gates_match_their_truth_tables() {
        let mut aig = Aig::new();
        let (a, b, c) = (aig.new_input(), aig.new_input(), aig.new_input());
        let gates = [
            aig.and(a, b),
            aig.or(a, b),
            aig.xor(a, b),
            aig.iff(a, b),
            aig.implies(a, b),
            aig.ite(a, b, c),
            aig.and_all([a, b, c]),
            aig.or_all([a, b, c]),
        ];
        for bits in 0..8u8 {
            let (va, vb, vc) = (bits & 1 == 1, bits & 2 == 2, bits & 4 == 4);
            let expected = [
                va && vb,
                va || vb,
                va != vb,
                va == vb,
                !va || vb,
                if va { vb } else { vc },
                va && vb && vc,
                va || vb || vc,
            ];
            let sim = aig.simulate(&[va, vb, vc]);
            for (gate, expected) in gates.iter().zip(expected) {
                assert_eq!(sim.value(*gate), expected, "inputs {va} {vb} {vc}");
            }
        }
    }
}
