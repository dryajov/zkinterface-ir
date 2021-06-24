use std::convert::TryFrom;
use std::error::Error;
use serde::{Deserialize, Serialize};

use crate::Result;
use flatbuffers::{FlatBufferBuilder, Vector, WIPOffset, ForwardsUOffset};
use crate::sieve_ir_generated::sieve_ir as g;
use crate::{WireId, Gate};

#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct Block(pub Vec<Gate>);

/// Convert from Flatbuffers references to owned structure.
impl<'a> TryFrom<g::Block<'a>> for Block {
    type Error = Box<dyn Error>;

    fn try_from(g_block: g::Block) -> Result<Block> {
        let ret = Block(
            Gate::try_from_vector(g_block.block().ok_or("Missing subcircuit in block")?)?
        );
        Ok(ret)
    }
}

impl Block {
    /// Serialize this structure into a Flatbuffer message
    pub fn build<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        &'args self,
        builder: &'mut_bldr mut FlatBufferBuilder<'bldr>,
    ) -> WIPOffset<g::Block<'bldr>> {
        let impl_gates = Gate::build_vector(builder, &self.0);
        g::Block::create(
            builder,
            &g::BlockArgs {
                block: Some(impl_gates),
            }
        )
    }


    /// Convert from Flatbuffers vector of directives into owned structure.
    pub fn try_from_vector<'a>(
        g_vector: Vector<'a, ForwardsUOffset<g::Block<'a>>>,
    ) -> Result<Vec<Block>> {
        let mut directives = vec![];
        for i in 0..g_vector.len() {
            let g_a = g_vector.get(i);
            directives.push(Block::try_from(g_a)?);
        }
        Ok(directives)
    }

    /// Add a vector of this structure into a Flatbuffers message builder.
    pub fn build_vector<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        builder: &'mut_bldr mut FlatBufferBuilder<'bldr>,
        directives: &'args [Block],
    ) -> WIPOffset<Vector<'bldr, ForwardsUOffset<g::Block<'bldr>>>> {
        let g_directives: Vec<_> = directives.iter().map(|directive| directive.build(builder)).collect();
        let g_vector = builder.create_vector(&g_directives);
        g_vector
    }
}

pub fn translate_gates<'s>(subcircuit: &'s[Gate], output_input_wires: &'s[WireId]) -> impl Iterator<Item = Gate> + 's {
    subcircuit
        .iter()
        .map(move |gate| translate_gate(gate, output_input_wires))
}

fn translate_gate(gate: &Gate, output_input_wires: &[WireId]) -> Gate {
    match gate {
        Gate::Constant(out, val) => Gate::Constant(output_input_wires[*out as usize], val.clone()),
        Gate::AssertZero(out) => Gate::AssertZero(output_input_wires[*out as usize]),
        Gate::Copy(out, inp) => Gate::Copy(output_input_wires[*out as usize], output_input_wires[*inp as usize]),
        Gate::Add(out, a, b) => Gate::Add(output_input_wires[*out as usize], output_input_wires[*a as usize], output_input_wires[*b as usize]),
        Gate::Mul(out, a, b) => Gate::Mul(output_input_wires[*out as usize], output_input_wires[*a as usize], output_input_wires[*b as usize]),
        Gate::AddConstant(out, a, val) => Gate::AddConstant(output_input_wires[*out as usize], output_input_wires[*a as usize], val.clone()),
        Gate::MulConstant(out, a, val) => Gate::MulConstant(output_input_wires[*out as usize], output_input_wires[*a as usize], val.clone()),
        Gate::And(out, a, b) => Gate::And(output_input_wires[*out as usize], output_input_wires[*a as usize], output_input_wires[*b as usize]),
        Gate::Xor(out, a, b) => Gate::Xor(output_input_wires[*out as usize], output_input_wires[*a as usize], output_input_wires[*b as usize]),
        Gate::Not(out, a) => Gate::Not(output_input_wires[*out as usize], output_input_wires[*a as usize]),
        Gate::Instance(out) => Gate::Instance(output_input_wires[*out as usize]),
        Gate::Witness(out) => Gate::Witness(output_input_wires[*out as usize]),
        Gate::Free(from, end) => Gate::Free(output_input_wires[*from as usize], end.map(|id| output_input_wires[id as usize])),

        Gate::Call(name, outs,ins) =>
            Gate::Call(name.clone(), translate_vector_wires(outs, output_input_wires), translate_vector_wires(ins, output_input_wires)),

        Gate::Switch(condition, output_wires, input_wires, instance_count, witness_count, cases, branches) =>
            Gate::Switch(
                output_input_wires[*condition as usize],
                translate_vector_wires(output_wires, output_input_wires),
                translate_vector_wires(input_wires, output_input_wires),
                *instance_count,
                *witness_count,
                cases.clone(),
                branches.iter().map(|branch| translate_block(branch, output_input_wires)).collect(),
            ),

        // This one should never happen
        Gate::Function(..) => panic!("Function should not be defined within bodies."),
    }
}

fn translate_vector_wires(wires: &[WireId], output_input_wires: &[WireId]) -> Vec<WireId> {
    wires.iter().map(|id| output_input_wires[*id as usize]).collect()
}

fn translate_block(block: &Block, output_input_wires: &[WireId]) -> Block {
    Block(
        translate_gates(&block.0, output_input_wires).collect()
    )
}