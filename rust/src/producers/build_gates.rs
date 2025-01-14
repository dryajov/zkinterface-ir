use serde::{Deserialize, Serialize};

use crate::producers::builder::SwitchParams;
use crate::structs::{function::CaseInvoke, wire::WireList};
use crate::{Gate, Value, WireId};

/// BuildGate is similar to Gate but without output wires.
/// Useful in combination with GateBuilder.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub enum BuildGate {
    Constant(Value),
    AssertZero(WireId),
    Copy(WireId),
    Add(WireId, WireId),
    Mul(WireId, WireId),
    AddConstant(WireId, Value),
    MulConstant(WireId, Value),
    And(WireId, WireId),
    Xor(WireId, WireId),
    Not(WireId),
    Instance(Option<Value>),
    Witness(Option<Value>),
    Free(WireId, Option<WireId>),
}

pub const NO_OUTPUT: WireId = WireId::MAX;

use std::hash::Hash;
use BuildGate::*;

impl BuildGate {
    pub fn with_output(self, output: WireId) -> Gate {
        match self {
            Constant(value) => Gate::Constant(output, value),
            AssertZero(input) => {
                assert_eq!(output, NO_OUTPUT);
                Gate::AssertZero(input)
            }
            Copy(input) => Gate::Copy(output, input),
            Add(left, right) => Gate::Add(output, left, right),
            Mul(left, right) => Gate::Mul(output, left, right),
            AddConstant(left, value) => Gate::AddConstant(output, left, value),
            MulConstant(left, value) => Gate::MulConstant(output, left, value),
            And(left, right) => Gate::And(output, left, right),
            Xor(left, right) => Gate::Xor(output, left, right),
            Not(input) => Gate::Not(output, input),
            Instance(_value) => Gate::Instance(output),
            Witness(_value) => Gate::Witness(output),
            Free(first, last) => {
                assert_eq!(output, NO_OUTPUT);
                Gate::Free(first, last)
            }
        }
    }

    pub fn has_output(&self) -> bool {
        match *self {
            AssertZero(_) => false,
            Free(_, _) => false,
            _ => true,
        }
    }
}

/// BuildComplexGate is similar to a complex Gate (Call, Switch or For) but without output wires.
/// Useful in combination with GateBuilder.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub enum BuildComplexGate {
    // Call(name, input_wires)
    Call(String, WireList),
    // Switch(condition, cases, branches, params)
    Switch(WireId, Vec<Value>, Vec<CaseInvoke>, SwitchParams),
}

use BuildComplexGate::*;

impl BuildComplexGate {
    pub fn with_output(self, output: WireList) -> Gate {
        match self {
            Call(name, input_wires) => Gate::Call(name, output, input_wires),
            Switch(condition, cases, branches, _) => {
                Gate::Switch(condition, output, cases, branches)
            }
        }
    }
}
