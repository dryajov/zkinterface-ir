use flatbuffers::{FlatBufferBuilder, WIPOffset};
use serde::{Deserialize, Serialize};

use crate::sieve_ir_generated::sieve_ir as g;
use crate::sieve_ir_generated::sieve_ir::GateSet as gs;
use super::{WireId, Value};


#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub enum Gate {
    Constant(WireId, Value),
    AssertZero(WireId),
    Copy(WireId, WireId),
    Add(WireId, WireId, WireId),
    Mul(WireId, WireId, WireId),
    AddConstant(WireId, WireId, Value),
    MulConstant(WireId, WireId, Value),
    And(WireId, WireId, WireId),
    Xor(WireId, WireId, WireId),
    Not(WireId, WireId),
}

use Gate::*;

impl<'a> From<g::Gate<'a>> for Gate {
    /// Convert from Flatbuffers references to owned structure.
    fn from(gen_gate: g::Gate) -> Gate {
        match gen_gate.gate_type() {
            gs::NONE => panic!("No gate type"),

            gs::GateConstant => {
                let gate = gen_gate.gate_as_gate_constant().unwrap();
                Constant(
                    gate.output().unwrap().id(),
                    Vec::from(gate.constant().unwrap()))
            }

            gs::GateAssertZero => {
                let gate = gen_gate.gate_as_gate_assert_zero().unwrap();
                AssertZero(
                    gate.input().unwrap().id())
            }

            gs::GateCopy => {
                let gate = gen_gate.gate_as_gate_copy().unwrap();
                Copy(
                    gate.output().unwrap().id(),
                    gate.input().unwrap().id())
            }

            gs::GateAdd => {
                let gate = gen_gate.gate_as_gate_add().unwrap();
                Add(
                    gate.output().unwrap().id(),
                    gate.left().unwrap().id(),
                    gate.right().unwrap().id())
            }

            gs::GateMul => {
                let gate = gen_gate.gate_as_gate_mul().unwrap();
                Mul(
                    gate.output().unwrap().id(),
                    gate.left().unwrap().id(),
                    gate.right().unwrap().id())
            }

            gs::GateAddConstant => {
                let gate = gen_gate.gate_as_gate_add_constant().unwrap();
                AddConstant(
                    gate.output().unwrap().id(),
                    gate.input().unwrap().id(),
                    Vec::from(gate.constant().unwrap()))
            }

            gs::GateMulConstant => {
                let gate = gen_gate.gate_as_gate_mul_constant().unwrap();
                MulConstant(
                    gate.output().unwrap().id(),
                    gate.input().unwrap().id(),
                    Vec::from(gate.constant().unwrap()))
            }

            gs::GateAnd => {
                let gate = gen_gate.gate_as_gate_and().unwrap();
                And(
                    gate.output().unwrap().id(),
                    gate.left().unwrap().id(),
                    gate.right().unwrap().id())
            }

            gs::GateXor => {
                let gate = gen_gate.gate_as_gate_xor().unwrap();
                Xor(
                    gate.output().unwrap().id(),
                    gate.left().unwrap().id(),
                    gate.right().unwrap().id())
            }

            gs::GateNot => {
                let gate = gen_gate.gate_as_gate_not().unwrap();
                Not(
                    gate.output().unwrap().id(),
                    gate.input().unwrap().id())
            }
        }
    }
}

impl Gate {
    /// Add this structure into a Flatbuffers message builder.
    pub fn build<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        &'args self,
        builder: &'mut_bldr mut FlatBufferBuilder<'bldr>,
    ) -> WIPOffset<g::Gate<'bldr>>
    {
        match self {
            Constant(output, constant) => {
                let constant = builder.create_vector(constant);
                let gate = g::GateConstant::create(builder, &g::GateConstantArgs {
                    output: Some(&g::Wire::new(*output)),
                    constant: Some(constant),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateConstant,
                    gate: Some(gate.as_union_value()),
                })
            }

            AssertZero(input) => {
                let gate = g::GateAssertZero::create(builder, &g::GateAssertZeroArgs {
                    input: Some(&g::Wire::new(*input)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateAssertZero,
                    gate: Some(gate.as_union_value()),
                })
            }

            Copy(output, input) => {
                let gate = g::GateCopy::create(builder, &g::GateCopyArgs {
                    output: Some(&g::Wire::new(*output)),
                    input: Some(&g::Wire::new(*input)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateCopy,
                    gate: Some(gate.as_union_value()),
                })
            }

            Add(output, left, right) => {
                let gate = g::GateAdd::create(builder, &g::GateAddArgs {
                    output: Some(&g::Wire::new(*output)),
                    left: Some(&g::Wire::new(*left)),
                    right: Some(&g::Wire::new(*right)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateAdd,
                    gate: Some(gate.as_union_value()),
                })
            }

            Mul(output, left, right) => {
                let gate = g::GateMul::create(builder, &g::GateMulArgs {
                    output: Some(&g::Wire::new(*output)),
                    left: Some(&g::Wire::new(*left)),
                    right: Some(&g::Wire::new(*right)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateMul,
                    gate: Some(gate.as_union_value()),
                })
            }

            AddConstant(output, input, constant) => {
                let constant = builder.create_vector(constant);
                let gate = g::GateAddConstant::create(builder, &g::GateAddConstantArgs {
                    output: Some(&g::Wire::new(*output)),
                    input: Some(&g::Wire::new(*input)),
                    constant: Some(constant),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateAddConstant,
                    gate: Some(gate.as_union_value()),
                })
            }

            MulConstant(output, input, constant) => {
                let constant = builder.create_vector(constant);
                let gate = g::GateMulConstant::create(builder, &g::GateMulConstantArgs {
                    output: Some(&g::Wire::new(*output)),
                    input: Some(&g::Wire::new(*input)),
                    constant: Some(constant),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateMulConstant,
                    gate: Some(gate.as_union_value()),
                })
            }

            And(output, left, right) => {
                let gate = g::GateAnd::create(builder, &g::GateAndArgs {
                    output: Some(&g::Wire::new(*output)),
                    left: Some(&g::Wire::new(*left)),
                    right: Some(&g::Wire::new(*right)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateAnd,
                    gate: Some(gate.as_union_value()),
                })
            }

            Xor(output, left, right) => {
                let gate = g::GateXor::create(builder, &g::GateXorArgs {
                    output: Some(&g::Wire::new(*output)),
                    left: Some(&g::Wire::new(*left)),
                    right: Some(&g::Wire::new(*right)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateXor,
                    gate: Some(gate.as_union_value()),
                })
            }

            Not(output, input) => {
                let gate = g::GateNot::create(builder, &g::GateNotArgs {
                    output: Some(&g::Wire::new(*output)),
                    input: Some(&g::Wire::new(*input)),
                });
                g::Gate::create(builder, &g::GateArgs {
                    gate_type: gs::GateNot,
                    gate: Some(gate.as_union_value()),
                })
            }
        }
    }
}