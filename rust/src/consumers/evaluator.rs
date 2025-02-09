use crate::structs::function::{CaseInvoke, ForLoopBody};
use crate::structs::iterators::evaluate_iterexpr_list;
use crate::structs::relation::{contains_feature, BOOL};
use crate::structs::wire::expand_wirelist;
use crate::{Gate, Header, Instance, Message, Relation, Result, WireId, Witness};
use num_bigint::BigUint;
use num_traits::identities::{One, Zero};
use std::collections::{HashMap, VecDeque};
use std::ops::{BitAnd, BitXor, Shr};

/// The `ZKBackend` trait should be implemented by any backend that wants to evaluate SIEVE IR circuits.
/// It has to define 2 types:
///  - `Wire`: represents a variable in the circuit. It should implement the `Clone` trait.
///  - `FieldElement`: represents elements of the underlying field. Mainly used when importing
///                    instances/witnesses from the corresponding pools.
/// see `PlaintextBackend` for an working example of an implementation.
pub trait ZKBackend {
    type Wire;
    /// Usually a big Integer type.
    type FieldElement: 'static + Clone;

    /// Imports a `Self::FieldElement` from a byte buffer, in little endian.
    /// If the buffer does not represent an element of the underlying field, then
    /// it returns an Err.
    fn from_bytes_le(val: &[u8]) -> Result<Self::FieldElement>;
    /// Set the underlying field of the running backend.
    /// If the field is not compatible with this ZKBackend, then it should return Err
    fn set_field(&mut self, modulus: &[u8], degree: u32, is_boolean: bool) -> Result<()>;

    /// Returns a `FieldElement` representing '1' in the underlying field.
    fn one(&self) -> Result<Self::FieldElement>;
    /// Returns a `FieldElement` representing '-1' in the underlying field.
    fn minus_one(&self) -> Result<Self::FieldElement>;
    /// Returns a `FieldElement` representing '0' in the underlying field.
    fn zero(&self) -> Result<Self::FieldElement>;

    // Returns a new instance of a given Wire id
    fn copy(&mut self, wire: &Self::Wire) -> Result<Self::Wire>;

    /// Imports a constant value into a new `Self::Wire`.
    fn constant(&mut self, val: Self::FieldElement) -> Result<Self::Wire>;
    /// This functions checks whether the given `Self::Wire` is indeed zero or not.
    /// Depending upon whether `self` is in prover, or verifier mode, this function should act
    /// accordingly. E.g. in prover mode, if the wire is not zero, then this function should
    /// return an Err.
    fn assert_zero(&mut self, wire: &Self::Wire) -> Result<()>;

    /// Adds two wires into a new wire.
    fn add(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire>;
    /// Multiplies two wires into a new wire.
    fn multiply(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire>;
    /// Adds a given wire by a constant `Self::FieldElement` into a new wire.
    fn add_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire>;
    /// Multiplies a given wire by a constant `Self::FieldElement` into a new wire.
    fn mul_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire>;

    /// Performs a boolean `and` between two wires. The result is stored in a new wire.
    fn and(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire>;
    /// Performs a boolean `xor` between two wires. The result is stored in a new wire.
    fn xor(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire>;
    /// Performs a boolean `not` on a given wire. The result is stored in a new wire.
    fn not(&mut self, a: &Self::Wire) -> Result<Self::Wire>;

    /// This functions declares a new instance variable owning the value given as parameter,
    /// which should be stored in a new wire.
    fn instance(&mut self, val: Self::FieldElement) -> Result<Self::Wire>;
    /// This functions declares a new witness variable owning the value given as parameter,
    /// which should be stored in a new wire.
    /// The value is given as a `Option`, because depending upon the type of this ZKBackend
    /// (prover / verifier), it should act differently.
    ///  - In prover mode, the witness should be provided, so the value should be `Some`.
    ///  - In verifier mode, the witness should normally not be provided (except maybe in test mode)
    /// Both cases should return a `Self::Wire` so the ZKBackend should have a specific wire value
    /// to handle it when in verifier mode.
    fn witness(&mut self, val: Option<Self::FieldElement>) -> Result<Self::Wire>;
}

/// Used to evaluate a 'multiplication' in either the arithmetic case or the boolean,
/// where it's replaced by an AND operation.
fn as_mul<B: ZKBackend>(
    backend: &mut B,
    a: &B::Wire,
    b: &B::Wire,
    is_bool: bool,
) -> Result<B::Wire> {
    if is_bool {
        backend.and(a, b)
    } else {
        backend.multiply(a, b)
    }
}

/// Used to evaluate an 'addition' in either the arithmetic case or the boolean,
/// where it's replaced by an XOR operation.
fn as_add<B: ZKBackend>(
    backend: &mut B,
    a: &B::Wire,
    b: &B::Wire,
    is_bool: bool,
) -> Result<B::Wire> {
    if is_bool {
        backend.xor(a, b)
    } else {
        backend.add(a, b)
    }
}

// Computes the 'negative' value. `minus_one_buf` must be provided and include a buffer with modulus minus one
fn as_negate<B: ZKBackend>(backend: &mut B, wire: &B::Wire, is_bool: bool) -> Result<B::Wire> {
    if is_bool {
        // negation in boolean field is identity
        backend.copy(wire)
    } else {
        backend.mul_constant(wire, backend.minus_one()?)
    }
}

// Computes an 'addition' with constant. Boolean fields are not supported.
fn as_add_one<B: ZKBackend>(backend: &mut B, wire: &B::Wire, is_bool: bool) -> Result<B::Wire> {
    if is_bool {
        // adding one in boolean field is not
        backend.not(wire)
    } else {
        backend.add_constant(wire, backend.one()?)
    }
}

/// This structure defines a function as defined in the circuit, but without the name.
/// It's mainly used to retrieve information from the name.
struct FunctionDeclaration {
    subcircuit: Vec<Gate>,
    instance_nbr: usize,
    witness_nbr: usize,
    output_count: usize,
    input_count: usize,
}

/// This structure is the core of IR evaluation. It is instantiated using a ZKBackend,
/// and will read the IR circuit, parses it, and calls the corresponding function from the
/// ZKBackend to evaluate each single operation.
/// It will inline functions, unroll loops, and multiplex switches.
///
/// # Example
/// ```
/// use zki_sieve::consumers::evaluator::{PlaintextBackend, Evaluator};
/// use zki_sieve::producers::examples::*;
///
/// let relation = example_relation();
/// let instance = example_instance();
/// let witness = example_witness();
///
/// let mut zkbackend = PlaintextBackend::default();
/// let mut simulator = Evaluator::default();
/// let _ = simulator.ingest_instance(&instance);
/// let _ = simulator.ingest_witness(&witness);
/// let _ = simulator.ingest_relation(&relation, &mut zkbackend);
/// ```
pub struct Evaluator<B: ZKBackend> {
    values: HashMap<WireId, B::Wire>,
    modulus: BigUint,
    instance_queue: VecDeque<B::FieldElement>,
    witness_queue: VecDeque<B::FieldElement>,
    is_boolean: bool,

    // name => (instance_nbr, witness_nbr, subcircuit)
    known_functions: HashMap<String, FunctionDeclaration>,

    verified_at_least_one_gate: bool,
    found_error: Option<String>,
}

impl<B: ZKBackend> Default for Evaluator<B> {
    fn default() -> Self {
        Evaluator {
            values: Default::default(),
            modulus: BigUint::zero(),
            instance_queue: Default::default(),
            witness_queue: Default::default(),
            is_boolean: false,
            known_functions: Default::default(),
            verified_at_least_one_gate: false,
            found_error: None,
        }
    }
}

impl<B: ZKBackend> Evaluator<B> {
    /// Creates an Evaluator for an iterator over `Messages`
    /// The returned Evaluator can then be reused to ingest more messages using the one of the
    /// `ingest_***` functions.
    pub fn from_messages(messages: impl Iterator<Item = Result<Message>>, backend: &mut B) -> Self {
        let mut evaluator = Evaluator::default();
        messages.for_each(|msg| evaluator.ingest_message(&msg.unwrap(), backend));
        evaluator
    }

    /// Returns the list of violations detected when evaluating the IR circuit.
    /// It consumes `self`.
    pub fn get_violations(self) -> Vec<String> {
        let mut violations = vec![];
        if !self.verified_at_least_one_gate {
            violations.push("Did not receive any gate to verify.".to_string());
        }
        if let Some(err) = self.found_error {
            violations.push(err);
        }
        violations
    }

    /// Ingests a `Message` using the ZKBackend given in parameter.
    /// If a error was found in previous Messages, then it does nothing but returns,
    /// otherwise it ingests the message.
    pub fn ingest_message(&mut self, msg: &Message, backend: &mut B) {
        if self.found_error.is_some() {
            return;
        }

        match self.ingest_message_(msg, backend) {
            Err(err) => self.found_error = Some(err.to_string()),
            Ok(()) => {}
        }
    }

    fn ingest_message_(&mut self, msg: &Message, backend: &mut B) -> Result<()> {
        match msg {
            Message::Instance(i) => self.ingest_instance(&i),
            Message::Witness(w) => self.ingest_witness(&w),
            Message::Relation(r) => self.ingest_relation(&r, backend),
        }
    }

    fn ingest_header(&mut self, header: &Header) -> Result<()> {
        self.modulus = BigUint::from_bytes_le(&header.field_characteristic);
        Ok(())
    }

    /// Ingest an `Instance` message, and returns a `Result` whether ot nor an error
    /// was encountered. It stores the instance values in a pool.
    pub fn ingest_instance(&mut self, instance: &Instance) -> Result<()> {
        self.ingest_header(&instance.header)?;

        for value in &instance.common_inputs {
            self.instance_queue.push_back(B::from_bytes_le(value)?);
        }
        Ok(())
    }

    /// Ingest an `Witness` message, and returns a `Result` whether ot nor an error
    /// was encountered. It stores the witness values in a pool.
    pub fn ingest_witness(&mut self, witness: &Witness) -> Result<()> {
        self.ingest_header(&witness.header)?;

        for value in &witness.short_witness {
            self.witness_queue.push_back(B::from_bytes_le(value)?);
        }
        Ok(())
    }

    /// Ingest a `Relation` message
    pub fn ingest_relation(&mut self, relation: &Relation, backend: &mut B) -> Result<()> {
        self.ingest_header(&relation.header)?;
        self.is_boolean = contains_feature(relation.gate_mask, BOOL);
        backend.set_field(
            &relation.header.field_characteristic,
            relation.header.field_degree,
            self.is_boolean,
        )?;

        if relation.gates.len() > 0 {
            self.verified_at_least_one_gate = true;
        }

        for f in relation.functions.iter() {
            self.known_functions.insert(
                f.name.clone(),
                FunctionDeclaration {
                    subcircuit: f.body.clone(),
                    instance_nbr: f.instance_count,
                    witness_nbr: f.witness_count,
                    output_count: f.output_count,
                    input_count: f.input_count,
                },
            );
        }

        let mut known_iterators = HashMap::new();

        for gate in &relation.gates {
            Self::ingest_gate(
                gate,
                backend,
                &mut self.values,
                &self.known_functions,
                &mut known_iterators,
                &self.modulus,
                self.is_boolean,
                &mut self.instance_queue,
                &mut self.witness_queue,
                None,
            )?;
        }
        Ok(())
    }

    /// This function ingests one gate at a time (but can call itself recursively)
    /// If the current gate is in a branch of a switch, then it has to be weighted.
    /// The weight is used in `AssertZero` gates by multiplying the tested wire by the weight. It
    /// allows to 'disable' assertions that are in a not-taken branch. The weight should propagate
    /// to inner gates.
    /// - `scope` contains the list of existing wires with their respective value. It will be
    ///    augmented if this gate produces outputs, or reduced if this is a `GateFree`
    /// - `known_functions` is the map of functions defined in previous or current `Relation` message
    /// - `known_iterators` is the map of defined iterators. It will be temporarily updated if the
    ///    current gate is a `GateFor`
    /// - `modulus` and `is_boolean` are used mainly is switches to compute the weight of each branch.
    /// - `instances` and `witnesses` are the instances and witnesses pools, implemented as Queues.
    ///    They will be consumed whenever necessary.
    fn ingest_gate(
        gate: &Gate,
        backend: &mut B,
        scope: &mut HashMap<WireId, B::Wire>,
        known_functions: &HashMap<String, FunctionDeclaration>,
        known_iterators: &mut HashMap<String, u64>,
        modulus: &BigUint,
        is_boolean: bool,
        instances: &mut VecDeque<B::FieldElement>,
        witnesses: &mut VecDeque<B::FieldElement>,
        weight: Option<&B::Wire>,
    ) -> Result<()> {
        use Gate::*;

        macro_rules! get {
            ($wire_id:expr) => {{
                get::<B>(scope, $wire_id)
            }};
        }

        macro_rules! set {
            ($wire_id:expr, $wire_name:expr) => {{
                set::<B>(scope, $wire_id, $wire_name)
            }};
        }

        match gate {
            Constant(out, value) => {
                let wire = backend.constant(B::from_bytes_le(value)?)?;
                set::<B>(scope, *out, wire)?;
            }

            AssertZero(inp) => {
                let inp_wire = get!(*inp)?;
                let should_be_zero = if let Some(w) = weight {
                    as_mul(backend, w, inp_wire, is_boolean)?
                } else {
                    backend.copy(&inp_wire)?
                };
                if backend.assert_zero(&should_be_zero).is_err() {
                    return Err(format!(
                        "Wire_{} (may be weighted) should be 0, while it is not",
                        *inp
                    )
                    .into());
                }
            }

            Copy(out, inp) => {
                let in_wire = get!(*inp)?;
                let out_wire = backend.copy(in_wire)?;
                set!(*out, out_wire)?;
            }

            Add(out, left, right) => {
                let l = get!(*left)?;
                let r = get!(*right)?;
                let sum = backend.add(l, r)?;
                set!(*out, sum)?;
            }

            Mul(out, left, right) => {
                let l = get!(*left)?;
                let r = get!(*right)?;
                let prod = backend.multiply(l, r)?;
                set!(*out, prod)?;
            }

            AddConstant(out, inp, constant) => {
                let l = get!(*inp)?;
                let r = B::from_bytes_le(constant)?;
                let sum = backend.add_constant(l, r)?;
                set!(*out, sum)?;
            }

            MulConstant(out, inp, constant) => {
                let l = get!(*inp)?;
                let r = B::from_bytes_le(constant)?;
                let prod = backend.mul_constant(l, r)?;
                set!(*out, prod)?;
            }

            And(out, left, right) => {
                let l = get!(*left)?;
                let r = get!(*right)?;
                let and = backend.and(l, r)?;
                set!(*out, and)?;
            }

            Xor(out, left, right) => {
                let l = get!(*left)?;
                let r = get!(*right)?;
                let xor = backend.xor(l, r)?;
                set!(*out, xor)?;
            }

            Not(out, inp) => {
                let val = get!(*inp)?;
                let not = backend.not(val)?;
                set!(*out, not)?;
            }

            Instance(out) => {
                let val = if let Some(inner) = instances.pop_front() {
                    inner
                } else {
                    return Err("Not enough instance to consume".into());
                };
                set_instance(backend, scope, *out, val)?;
            }

            Witness(out) => {
                let val = witnesses.pop_front();
                set_witness(backend, scope, *out, val)?;
            }

            Free(first, last) => {
                let last_value = last.unwrap_or(*first);
                for current in *first..=last_value {
                    remove::<B>(scope, current)?;
                }
            }

            Call(name, output_wires, input_wires) => {
                let function = known_functions
                    .get(name)
                    .ok_or_else(|| "Unknown function")?;
                let expanded_output = expand_wirelist(output_wires)?;
                let expanded_input = expand_wirelist(input_wires)?;

                // simple checks.
                if expanded_output.len() != function.output_count {
                    return Err(format!("Wrong number of output variables in call to function {} (Expected {} / Got {}).", name, function.output_count, expanded_output.len()).into());
                }
                if expanded_input.len() != function.input_count {
                    return Err(format!("Wrong number of input variables in call to function {} (Expected {} / Got {}).", name, function.input_count, expanded_input.len()).into());
                }

                // in the case of an named call, iterators *ARE NOT* forwarded into inner bodies.
                Self::ingest_subcircuit(
                    &function.subcircuit,
                    backend,
                    &expanded_output,
                    &expanded_input,
                    scope,
                    known_functions,
                    &mut HashMap::new(),
                    modulus,
                    is_boolean,
                    instances,
                    witnesses,
                    weight,
                )?;
            }

            AnonCall(output_wires, input_wires, _, _, subcircuit) => {
                let expanded_output = expand_wirelist(output_wires)?;
                let expanded_input = expand_wirelist(input_wires)?;
                // in the case of an anoncall, iterators *ARE* forwarded into inner bodies.
                Self::ingest_subcircuit(
                    subcircuit,
                    backend,
                    &expanded_output,
                    &expanded_input,
                    scope,
                    known_functions,
                    known_iterators,
                    modulus,
                    is_boolean,
                    instances,
                    witnesses,
                    weight,
                )?;
            }

            // For loops are unrolled. The body is called as many times (NB: the loop bounds are
            // inclusive), and iterator expressions are evaluated for each.
            For(iterator_name, start_val, end_val, _, body) => {
                for i in *start_val..=*end_val {
                    known_iterators.insert(iterator_name.clone(), i);

                    match body {
                        ForLoopBody::IterExprCall(name, outputs, inputs) => {
                            let function = known_functions
                                .get(name)
                                .ok_or_else(|| "Unknown function")?;
                            let expanded_output = evaluate_iterexpr_list(outputs, known_iterators);
                            let expanded_input = evaluate_iterexpr_list(inputs, known_iterators);

                            // simple checks.
                            if expanded_output.len() != function.output_count {
                                return Err(format!("Wrong number of output variables in call to function {} (Expected {} / Got {}).", name, function.output_count, expanded_output.len()).into());
                            }
                            if expanded_input.len() != function.input_count {
                                return Err(format!("Wrong number of input variables in call to function {} (Expected {} / Got {}).", name, function.input_count, expanded_input.len()).into());
                            }

                            Self::ingest_subcircuit(
                                &function.subcircuit,
                                backend,
                                &expanded_output,
                                &expanded_input,
                                scope,
                                known_functions,
                                &mut HashMap::new(),
                                modulus,
                                is_boolean,
                                instances,
                                witnesses,
                                weight,
                            )?;
                        }
                        ForLoopBody::IterExprAnonCall(
                            output_wires,
                            input_wires,
                            _,
                            _,
                            subcircuit,
                        ) => {
                            let expanded_output =
                                evaluate_iterexpr_list(output_wires, known_iterators);
                            let expanded_input =
                                evaluate_iterexpr_list(input_wires, known_iterators);
                            Self::ingest_subcircuit(
                                subcircuit,
                                backend,
                                &expanded_output,
                                &expanded_input,
                                scope,
                                known_functions,
                                known_iterators,
                                modulus,
                                is_boolean,
                                instances,
                                witnesses,
                                weight,
                            )?;
                        }
                    }
                }
                known_iterators.remove(iterator_name);
            }

            // Switches are multiplexed. Each branch has a weight, which is (in plaintext) either 1
            // or 0 (0 if the branch is not taken, 1 otherwise)
            Switch(condition, output_wires, cases, branches) => {
                // determine the maximum instance/witness consumption
                let mut max_instance_count: usize = 0;
                let mut max_witness_count: usize = 0;
                for branch in branches.iter() {
                    let (instance_cnt, witness_cnt) = match branch {
                        CaseInvoke::AbstractGateCall(name, _) => {
                            let function = known_functions
                                .get(name)
                                .ok_or_else(|| "Unknown function")?;
                            (function.instance_nbr, function.witness_nbr)
                        }
                        CaseInvoke::AbstractAnonCall(_, instance_count, witness_count, _) => {
                            (*instance_count, *witness_count)
                        }
                    };
                    max_instance_count = std::cmp::max(max_instance_count, instance_cnt);
                    max_witness_count = std::cmp::max(max_witness_count, witness_cnt);
                }

                // 'consumes' max_instances and max_witnesses values from the corresponding pools
                // by removing them from the instances/witnesses variables, and storing them into
                // new queues. The new queues (a clone of them) will be used in each branch.
                let mut new_instances: VecDeque<B::FieldElement> =
                    instances.split_off(std::cmp::min(instances.len(), max_instance_count));
                std::mem::swap(instances, &mut new_instances);
                let mut new_witnesses: VecDeque<B::FieldElement> =
                    witnesses.split_off(std::cmp::min(witnesses.len(), max_witness_count));
                std::mem::swap(witnesses, &mut new_witnesses);

                // This will handle the input/output wires for each branches. Output wires will then
                // be combined using their respective weight.
                let mut branches_scope = Vec::new();

                let expanded_output = expand_wirelist(output_wires)?;
                let mut weights = Vec::new();

                for (case, branch) in cases.iter().zip(branches.iter()) {
                    // Compute (1 - ('case' - 'condition') ^ (self.modulus - 1))
                    let branch_weight =
                        compute_weight(backend, case, get!(*condition)?, modulus, is_boolean)?;
                    let weighted_branch_weight = if let Some(w) = weight {
                        as_mul(backend, w, &branch_weight, is_boolean)?
                    } else {
                        branch_weight
                    };

                    let mut branch_scope = HashMap::new();
                    match branch {
                        CaseInvoke::AbstractGateCall(name, input_wires) => {
                            let function = known_functions
                                .get(name)
                                .ok_or_else(|| format!("Unknown function: {}", name))?;
                            let expanded_input = expand_wirelist(input_wires)?;

                            // simple checks.
                            if expanded_output.len() != function.output_count {
                                return Err(format!("Wrong number of output variables in call to function {} (Expected {} / Got {}).", name, function.output_count, expanded_output.len()).into());
                            }
                            if expanded_input.len() != function.input_count {
                                return Err(format!("Wrong number of input variables in call to function {} (Expected {} / Got {}).", name, function.input_count, expanded_input.len()).into());
                            }

                            for wire in expanded_input.iter() {
                                let w = get!(*wire)?;
                                branch_scope.insert(*wire, backend.copy(w)?);
                            }
                            Self::ingest_subcircuit(
                                &function.subcircuit,
                                backend,
                                &expanded_output,
                                &expanded_input,
                                &mut branch_scope,
                                known_functions,
                                &mut HashMap::new(),
                                modulus,
                                is_boolean,
                                &mut new_instances.clone(),
                                &mut new_witnesses.clone(),
                                Some(&weighted_branch_weight),
                            )?;
                        }
                        CaseInvoke::AbstractAnonCall(input_wires, _, _, subcircuit) => {
                            let expanded_input = expand_wirelist(input_wires)?;
                            for wire in expanded_input.iter() {
                                let w = get!(*wire)?;
                                branch_scope.insert(*wire, backend.copy(w)?);
                            }
                            Self::ingest_subcircuit(
                                subcircuit,
                                backend,
                                &expanded_output,
                                &expanded_input,
                                &mut branch_scope,
                                known_functions,
                                known_iterators,
                                modulus,
                                is_boolean,
                                &mut new_instances.clone(),
                                &mut new_witnesses.clone(),
                                Some(&weighted_branch_weight),
                            )?;
                        }
                    }
                    weights.push(weighted_branch_weight);
                    // TODO we don't need all the scope here, only the output wires.
                    branches_scope.push(branch_scope);
                }

                // Compute the weighted sum for all output wire.
                for output_wire in expanded_output.iter() {
                    let weighted_output = branches_scope.iter().zip(weights.iter()).fold(
                        backend.constant(backend.zero()?),
                        |accu, (branch_scope, branch_weight)| {
                            let weighted_wire = as_mul(
                                backend,
                                get::<B>(branch_scope, *output_wire)?,
                                branch_weight,
                                is_boolean,
                            )?;
                            as_add(backend, &accu?, &weighted_wire, is_boolean)
                        },
                    )?;
                    set!(*output_wire, weighted_output)?;
                }
            }
        }
        Ok(())
    }

    /// This function is similar to `ingest_gate` except that it operates linearly on a subcircuit
    /// (i.e. a list of gates in an inner body of another gate).
    /// It will operate on an internal `scope`, and will write outputs produced by the subcircuit
    /// into the caller scope.
    /// Internally, it will call `ingest_gate` with the internal scope for each sub-gate.
    fn ingest_subcircuit(
        subcircuit: &[Gate],
        backend: &mut B,
        output_list: &[WireId],
        input_list: &[WireId],
        scope: &mut HashMap<WireId, B::Wire>,
        known_functions: &HashMap<String, FunctionDeclaration>,
        known_iterators: &mut HashMap<String, u64>,
        modulus: &BigUint,
        is_boolean: bool,
        instances: &mut VecDeque<B::FieldElement>,
        witnesses: &mut VecDeque<B::FieldElement>,
        weight: Option<&B::Wire>,
    ) -> Result<()> {
        let mut new_scope: HashMap<WireId, B::Wire> = HashMap::new();

        // copy the inputs required by this function into the new scope, at the proper index
        for (idx, input) in input_list.iter().enumerate() {
            let i = get::<B>(scope, *input)?;
            set::<B>(
                &mut new_scope,
                (idx + output_list.len()) as u64,
                backend.copy(i)?,
            )?;
        }
        // evaluate the subcircuit in the new scope.
        for gate in subcircuit {
            Self::ingest_gate(
                gate,
                backend,
                &mut new_scope,
                known_functions,
                known_iterators,
                modulus,
                is_boolean,
                instances,
                witnesses,
                weight,
            )?;
        }
        // copy the outputs produced from 'new_scope', into 'scope'

        for (idx, output) in output_list.iter().enumerate() {
            let w = get::<B>(&new_scope, idx as u64)?;
            set::<B>(scope, *output, backend.copy(w)?)?
        }

        Ok(())
    }

    /// This helper function can be used to retrieve value of a given wire at some point
    /// if it has *NOT* been freed yet, otherwise it will return an Err.
    pub fn get(&self, id: WireId) -> Result<&B::Wire> {
        get::<B>(&self.values, id)
    }
}

fn set_instance<I: ZKBackend>(
    backend: &mut I,
    scope: &mut HashMap<WireId, I::Wire>,
    id: WireId,
    value: I::FieldElement,
) -> Result<()> {
    let wire = backend.instance(value)?;
    set::<I>(scope, id, wire)
}

fn set_witness<I: ZKBackend>(
    backend: &mut I,
    scope: &mut HashMap<WireId, I::Wire>,
    id: WireId,
    value: Option<I::FieldElement>,
) -> Result<()> {
    let wire = backend.witness(value)?;
    set::<I>(scope, id, wire)
}

fn set<I: ZKBackend>(
    scope: &mut HashMap<WireId, I::Wire>,
    id: WireId,
    wire: I::Wire,
) -> Result<()> {
    if scope.insert(id, wire).is_some() {
        Err(format!("Wire_{} already has a value in this scope.", id).into())
    } else {
        Ok(())
    }
}

pub fn get<I: ZKBackend>(scope: &HashMap<WireId, I::Wire>, id: WireId) -> Result<&I::Wire> {
    scope
        .get(&id)
        .ok_or_else(|| format!("No value given for wire_{}", id).into())
}

fn remove<I: ZKBackend>(scope: &mut HashMap<WireId, I::Wire>, id: WireId) -> Result<I::Wire> {
    scope
        .remove(&id)
        .ok_or_else(|| format!("No value given for wire_{}", id).into())
}

/// This function will compute the modular exponentiation of a given base to a given exponent
/// recursively. It returns the wire holding the result.
fn exp<I: ZKBackend>(
    backend: &mut I,
    base: &I::Wire,
    exponent: &BigUint,
    modulus: &BigUint,
    is_boolean: bool,
) -> Result<I::Wire> {
    if exponent.is_one() {
        return backend.copy(base);
    }

    let previous = exp(backend, base, &exponent.shr(1), modulus, is_boolean)?;

    let ret = as_mul(backend, &previous, &previous, is_boolean);
    if exponent.bitand(BigUint::one()).is_one() {
        as_mul(backend, &ret?, base, is_boolean)
    } else {
        ret
    }
}

/// This function will compute '1 - (case - condition)^(p-1)' using a bunch of mul/and add/xor addc/xorc gates
fn compute_weight<B: ZKBackend>(
    backend: &mut B,
    case: &[u8],
    condition: &B::Wire,
    modulus: &BigUint,
    is_boolean: bool,
) -> Result<B::Wire> {
    let case_wire = &backend.constant(B::from_bytes_le(case)?)?;
    // scalar value
    let exponent = modulus - &BigUint::one();

    let minus_cond = &as_negate(backend, condition, is_boolean)?;
    let base = &as_add(backend, case_wire, minus_cond, is_boolean)?;
    let base_to_exp = &exp(backend, base, &exponent, modulus, is_boolean)?;
    let right = &as_negate(backend, base_to_exp, is_boolean)?;
    as_add_one(backend, right, is_boolean)
}

/// This is the default backend, evaluating a IR circuit in plaintext, meaning that it is not meant
/// for security purposes, will never ensure ZK properties, ...
/// It's used only for demo or tests.
/// Moreover, it's not optimized at all for modular operations (e.g. modular multiplications) and
/// can even be slower than a secure backend if the evaluated circuit contains a lot of such
/// operations.
/// Currently, this backend does not support 'verifier' mode, and requires witnesses to be provided.
pub struct PlaintextBackend {
    pub m: BigUint,
}

impl Default for PlaintextBackend {
    fn default() -> Self {
        PlaintextBackend { m: BigUint::zero() }
    }
}

impl ZKBackend for PlaintextBackend {
    type Wire = BigUint;
    type FieldElement = BigUint;

    fn from_bytes_le(val: &[u8]) -> Result<Self::FieldElement> {
        Ok(BigUint::from_bytes_le(val))
    }

    fn set_field(&mut self, modulus: &[u8], degree: u32, _is_boolean: bool) -> Result<()> {
        self.m = BigUint::from_bytes_le(modulus);
        if self.m.is_zero() {
            Err("Modulus cannot be zero.".into())
        } else if degree != 1 {
            Err("Field should be of degree 1".into())
        } else {
            Ok(())
        }
    }

    fn one(&self) -> Result<Self::FieldElement> {
        Ok(BigUint::one())
    }

    fn minus_one(&self) -> Result<Self::FieldElement> {
        if self.m.is_zero() {
            return Err("Modulus is not initiated, used `set_field()` before calling.".into());
        }
        Ok(&self.m - self.one()?)
    }

    fn zero(&self) -> Result<Self::FieldElement> {
        Ok(BigUint::zero())
    }

    fn copy(&mut self, wire: &Self::Wire) -> Result<Self::Wire> {
        Ok(wire.clone())
    }

    fn constant(&mut self, val: Self::FieldElement) -> Result<Self::Wire> {
        Ok(val)
    }

    fn assert_zero(&mut self, wire: &Self::Wire) -> Result<()> {
        if wire.is_zero() {
            Ok(())
        } else {
            Err("AssertZero failed".into())
        }
    }

    fn add(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        Ok((a + b) % &self.m)
    }

    fn multiply(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        Ok((a * b) % &self.m)
    }

    fn add_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire> {
        Ok((a + b) % &self.m)
    }

    fn mul_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire> {
        Ok((a * b) % &self.m)
    }

    fn and(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        Ok((a.bitand(b)) % &self.m)
    }

    fn xor(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        Ok((a.bitxor(b)) % &self.m)
    }

    fn not(&mut self, a: &Self::Wire) -> Result<Self::Wire> {
        Ok(if a.is_zero() {
            BigUint::one()
        } else {
            BigUint::zero()
        })
    }

    fn instance(&mut self, val: Self::FieldElement) -> Result<Self::Wire> {
        self.constant(val)
    }

    fn witness(&mut self, val: Option<Self::FieldElement>) -> Result<Self::Wire> {
        self.constant(val.unwrap_or_else(|| panic!("Missing witness value for PlaintextBackend")))
    }
}

#[test]
fn test_exponentiation() -> Result<()> {
    use itertools::izip;

    let mut backend = PlaintextBackend::default();

    let moduli = vec![
        BigUint::from(16249742125730185677094195492597105093u128),
        BigUint::from(101u64),
    ];

    let bases = vec![BigUint::from(2u128), BigUint::from(42u64)];

    let exponents = vec![
        BigUint::from(2206000150907221872269901214599500635u128),
        BigUint::from(100u64),
    ];

    let expecteds = vec![
        BigUint::from(5834907326474057072663503101785122138u128),
        BigUint::one(),
    ];

    for (modulus, base, exponent, expected) in izip!(
        moduli.iter(),
        bases.iter(),
        exponents.iter(),
        expecteds.iter()
    ) {
        backend.set_field(&modulus.to_bytes_le(), 1, false)?;
        let result = exp(&mut backend, base, exponent, modulus, false)?;
        assert_eq!(result, *expected);
    }

    Ok(())
}

#[test]
fn test_evaluator() -> crate::Result<()> {
    use crate::consumers::evaluator::Evaluator;
    use crate::producers::examples::*;

    let relation = example_relation();
    let instance = example_instance();
    let witness = example_witness();

    let mut zkbackend = PlaintextBackend::default();
    let mut simulator = Evaluator::default();
    simulator.ingest_instance(&instance)?;
    simulator.ingest_witness(&witness)?;
    simulator.ingest_relation(&relation, &mut zkbackend)?;

    assert_eq!(simulator.get_violations().len(), 0);

    Ok(())
}

#[test]
fn test_evaluator_as_verifier() -> crate::Result<()> {
    use crate::consumers::evaluator::Evaluator;
    /// This test simply checks that the Evaluator code could run with any ZKInterpreter without issue
    use crate::producers::examples::*;

    let relation = example_relation();
    let instance = example_instance();

    struct VerifierInterpreter {}
    impl ZKBackend for VerifierInterpreter {
        type Wire = i64;
        type FieldElement = BigUint;
        fn from_bytes_le(_val: &[u8]) -> Result<Self::FieldElement> {
            Ok(BigUint::zero())
        }
        fn set_field(&mut self, _modulus: &[u8], _degree: u32, _is_boolean: bool) -> Result<()> {
            Ok(())
        }
        fn one(&self) -> Result<Self::FieldElement> {
            Ok(BigUint::one())
        }
        fn zero(&self) -> Result<Self::FieldElement> {
            Ok(BigUint::zero())
        }
        fn minus_one(&self) -> Result<Self::FieldElement> {
            Ok(BigUint::one())
        }
        fn copy(&mut self, wire: &Self::Wire) -> Result<Self::Wire> {
            Ok(*wire)
        }
        fn constant(&mut self, _val: Self::FieldElement) -> Result<Self::Wire> {
            Ok(0)
        }
        fn assert_zero(&mut self, _wire: &Self::Wire) -> Result<()> {
            Ok(())
        }
        fn add(&mut self, _a: &Self::Wire, _b: &Self::Wire) -> Result<Self::Wire> {
            Ok(0)
        }
        fn multiply(&mut self, _a: &Self::Wire, _b: &Self::Wire) -> Result<Self::Wire> {
            Ok(0)
        }
        fn add_constant(&mut self, _a: &Self::Wire, _b: Self::FieldElement) -> Result<Self::Wire> {
            Ok(0)
        }
        fn mul_constant(&mut self, _a: &Self::Wire, _b: Self::FieldElement) -> Result<Self::Wire> {
            Ok(0)
        }
        fn and(&mut self, _a: &Self::Wire, _b: &Self::Wire) -> Result<Self::Wire> {
            Ok(0)
        }
        fn xor(&mut self, _a: &Self::Wire, _b: &Self::Wire) -> Result<Self::Wire> {
            Ok(0)
        }
        fn not(&mut self, _a: &Self::Wire) -> Result<Self::Wire> {
            Ok(0)
        }
        fn instance(&mut self, _val: Self::FieldElement) -> Result<Self::Wire> {
            Ok(0)
        }
        fn witness(&mut self, _val: Option<Self::FieldElement>) -> Result<Self::Wire> {
            Ok(0)
        }
    }

    let mut zkbackend = VerifierInterpreter {};
    let mut simulator = Evaluator::default();
    simulator.ingest_instance(&instance)?;
    simulator.ingest_relation(&relation, &mut zkbackend)?;

    assert_eq!(simulator.get_violations().len(), 0);

    Ok(())
}

#[test]
fn test_evaluator_wrong_result() -> crate::Result<()> {
    use crate::consumers::evaluator::Evaluator;
    use crate::producers::examples::*;

    let relation = example_relation();
    let instance = example_instance();
    let witness = example_witness_incorrect();

    let mut zkbackend = PlaintextBackend::default();
    let mut simulator = Evaluator::default();
    let _ = simulator.ingest_instance(&instance);
    let _ = simulator.ingest_witness(&witness);
    let should_be_err = simulator.ingest_relation(&relation, &mut zkbackend);

    assert!(should_be_err.is_err());
    assert_eq!(
        "Wire_9 (may be weighted) should be 0, while it is not",
        should_be_err.err().unwrap().to_string()
    );

    Ok(())
}
