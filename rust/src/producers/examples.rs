use flatbuffers::{emplace_scalar, read_scalar, EndianScalar};
use std::mem::size_of;

use crate::{Header, Instance, Relation, Witness};
use crate::structs::relation::{ADD, MUL, FUNCTION, SWITCH};

pub fn example_header() -> Header {
    example_header_in_field(literal32(EXAMPLE_MODULUS))
}

pub fn example_instance() -> Instance {
    example_instance_h(&example_header())
}

pub fn example_witness() -> Witness {
    example_witness_h(&example_header())
}

pub fn example_witness_incorrect() -> Witness {
    example_witness_incorrect_h(&example_header())
}

pub fn example_relation() -> Relation {
    example_relation_h(&example_header())
}

pub fn example_header_in_field(field_order: Vec<u8>) -> Header {
    Header {
        field_characteristic: field_order,
        ..Header::default()
    }
}

pub fn example_instance_h(header: &Header) -> Instance {
    Instance {
        header: header.clone(),
        common_inputs: vec![literal32(25)],
    }
}

pub fn example_witness_h(header: &Header) -> Witness {
    Witness {
        header: header.clone(),
        short_witness: vec![literal32(3), literal32(4)],
    }
}

pub fn example_witness_incorrect_h(header: &Header) -> Witness {
    Witness {
        header: header.clone(),
        short_witness: vec![
            literal32(3),
            literal32(4 + 1), // incorrect.
        ],
    }
}

pub fn example_relation_h(header: &Header) -> Relation {
    use crate::Gate::*;

    Relation {
        header: header.clone(),
        gate_mask: ADD|MUL,
        feat_mask: FUNCTION|SWITCH,
        gates: vec![
            Function("example/mul".to_string(), 1, 2, 0, 0, vec![Mul(0, 1, 2)]), // mul gate with ref implementation id1*id2 = id0
            Witness(1),
            Switch(
                1,                      // condition
                vec![0, 2, 4, 5, 6],    // output wires
                vec![1],
                1,
                1,
                vec![vec![3], vec![5]], // cases
                vec![
                    // branches
                    vec![
                        Instance(0),  // In Global Namespace: Instance(0)
                        Witness(1),   // In Global Namespace: Witness(2)
                        Call("example/mul".to_string(), vec![2], vec![5, 5]), // In Global Namespace: Mul(4, 1, 1)
                        Call("example/mul".to_string(), vec![3], vec![1, 1]), // In Global Namespace: Mul(5, 2, 2)
                        Add(4, 2, 3), // In Global Namespace: Add(6, 4, 5)
                    ],
                    // remapping local-to-global namespaces: [0, 2, 4, 5, 6] || [1] = [0, 2, 4, 5, 6, 1]
                    vec![
                        Instance(0),
                        Call("example/mul".to_string(), vec![1], vec![5, 0]),
                        Witness(2),
                        Mul(3, 1, 2),
                        Add(4, 2, 3),
                    ],
                ],
            ),
            Constant(3, encode_negative_one(&example_header())), // -1
            Call("example/mul".to_string(), vec![7], vec![3, 0]), // - instance_0
            Add(8, 6, 7),                                        // sum - instance_0
            Free(0, Some(7)),                                    // Free all previous wires
            AssertZero(8),                                       // difference == 0
        ],
    }
}

pub const EXAMPLE_MODULUS: u32 = 101;

pub fn literal<T: EndianScalar>(value: T) -> Vec<u8> {
    let mut buf = vec![0u8; size_of::<T>()];
    emplace_scalar(&mut buf[..], value);
    buf
}

fn literal32(v: u32) -> Vec<u8> {
    literal(v)
}

pub fn read_literal<T: EndianScalar>(encoded: &[u8]) -> T {
    if encoded.len() >= size_of::<T>() {
        read_scalar(encoded)
    } else {
        let mut encoded = Vec::from(encoded);
        encoded.resize(size_of::<T>(), 0);
        read_scalar(&encoded)
    }
}

pub fn encode_negative_one(header: &Header) -> Vec<u8> {
    let mut neg_one = header.field_characteristic.clone();
    assert!(neg_one.len() > 0 && neg_one[0] > 0, "Invalid field order");
    neg_one[0] -= 1;
    neg_one
}

#[test]
fn test_examples() {
    use crate::Source;

    let mut common_buf = Vec::<u8>::new();
    example_instance().write_into(&mut common_buf).unwrap();
    example_relation().write_into(&mut common_buf).unwrap();

    let mut prover_buf = Vec::<u8>::new();
    example_witness().write_into(&mut prover_buf).unwrap();

    let source = Source::from_buffers(vec![common_buf, prover_buf]);
    let messages = source.read_all_messages().unwrap();
    assert_eq!(messages.relations, vec![example_relation()]);
    assert_eq!(messages.instances, vec![example_instance()]);
    assert_eq!(messages.witnesses, vec![example_witness()]);
}
