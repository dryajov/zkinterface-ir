use crate::consumers::evaluator::ZKBackend;
use crate::producers::build_gates::BuildGate;
use crate::producers::builder::{GateBuilder, GateBuilderT};
use crate::structs::relation::{ARITH, BOOL, SIMPLE};
use crate::structs::IR_VERSION;
use crate::{Header, Result, Sink, Value, WireId};
use num_bigint::BigUint;
use num_traits::{One, Zero};

// TODO instead of using WireId, use something implementing Drop, which will call the corresponding
// Free gate when the wire is no more needed.

#[derive(Default)]
pub struct IRFlattener<S: Sink> {
    sink: Option<S>,
    b: Option<GateBuilder<S>>,
    modulus: BigUint,
}

impl<S: Sink> IRFlattener<S> {
    pub fn new(sink: S) -> Self {
        IRFlattener {
            sink: Some(sink),
            b: None,
            modulus: BigUint::zero(),
        }
    }

    pub fn finish(mut self) -> S {
        self.b.take().unwrap().finish()
    }
}

impl<S: Sink> Drop for IRFlattener<S> {
    fn drop(&mut self) {
        if self.b.is_some() {
            self.b.take().unwrap().finish();
        }
    }
}

impl<S: Sink> ZKBackend for IRFlattener<S> {
    type Wire = WireId;
    type FieldElement = BigUint;

    fn from_bytes_le(val: &[u8]) -> Result<Self::FieldElement> {
        Ok(BigUint::from_bytes_le(val))
    }

    fn set_field(&mut self, modulus: &[u8], degree: u32, is_boolean: bool) -> Result<()> {
        if self.b.is_none() {
            let header = Header {
                version: IR_VERSION.parse().unwrap(),
                field_characteristic: Value::from(modulus),
                field_degree: degree,
            };
            self.modulus = BigUint::from_bytes_le(modulus);
            self.b = Some(GateBuilder::new(
                self.sink.take().unwrap(),
                header,
                if is_boolean { BOOL } else { ARITH },
                SIMPLE,
            ));
        }
        Ok(())
    }

    fn one(&self) -> Result<Self::FieldElement> {
        Ok(BigUint::one())
    }

    fn minus_one(&self) -> Result<Self::FieldElement> {
        if self.modulus.is_zero() {
            return Err("Modulus is not initiated, used `set_field()` before calling.".into());
        }
        Ok(&self.modulus - self.one()?)
    }

    fn zero(&self) -> Result<Self::FieldElement> {
        Ok(BigUint::zero())
    }

    fn copy(&mut self, wire: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::Copy(*wire)))
    }

    fn constant(&mut self, val: Self::FieldElement) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self
            .b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::Constant(val.to_bytes_le())))
    }

    fn assert_zero(&mut self, wire: &Self::Wire) -> Result<()> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        self.b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::AssertZero(*wire));
        Ok(())
    }

    fn add(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::Add(*a, *b)))
    }

    fn multiply(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::Mul(*a, *b)))
    }

    fn add_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self
            .b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::AddConstant(*a, b.to_bytes_le())))
    }

    fn mul_constant(&mut self, a: &Self::Wire, b: Self::FieldElement) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self
            .b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::MulConstant(*a, b.to_bytes_le())))
    }

    fn and(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::And(*a, *b)))
    }

    fn xor(&mut self, a: &Self::Wire, b: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::Xor(*a, *b)))
    }

    fn not(&mut self, a: &Self::Wire) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self.b.as_mut().unwrap().create_gate(BuildGate::Not(*a)))
    }

    fn instance(&mut self, val: Self::FieldElement) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        Ok(self
            .b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::Instance(Some(val.to_bytes_le()))))
    }

    fn witness(&mut self, val: Option<Self::FieldElement>) -> Result<Self::Wire> {
        if self.b.is_none() {
            panic!("Builder has not been properly initialized.");
        }
        let value = val.map(|v| v.to_bytes_le());
        Ok(self
            .b
            .as_mut()
            .unwrap()
            .create_gate(BuildGate::Witness(value)))
    }
}

#[test]
fn test_validate_flattening() -> crate::Result<()> {
    use crate::consumers::evaluator::Evaluator;
    use crate::consumers::validator::Validator;
    use crate::producers::examples::*;
    use crate::producers::sink::MemorySink;
    use crate::Source;

    let instance = example_instance();
    let witness = example_witness();
    let relation = example_relation();

    let mut flattener = IRFlattener::new(MemorySink::default());
    let mut evaluator = Evaluator::default();

    evaluator.ingest_instance(&instance)?;
    evaluator.ingest_witness(&witness)?;
    evaluator.ingest_relation(&relation, &mut flattener)?;

    let s: Source = flattener.finish().into();

    let mut val = Validator::new_as_prover();
    for message in s.iter_messages() {
        val.ingest_message(&message?);
    }

    let violations = val.get_violations();

    assert_eq!(violations, Vec::<String>::new());

    Ok(())
}

#[test]
fn test_evaluate_flattening() -> crate::Result<()> {
    use crate::consumers::evaluator::{Evaluator, PlaintextBackend};
    use crate::producers::examples::*;
    use crate::producers::sink::MemorySink;
    use crate::Source;

    let relation = example_relation();
    let instance = example_instance();
    let witness = example_witness();

    let mut flattener = IRFlattener::new(MemorySink::default());
    let mut evaluator = Evaluator::default();

    evaluator.ingest_instance(&instance)?;
    evaluator.ingest_witness(&witness)?;
    evaluator.ingest_relation(&relation, &mut flattener)?;

    let s: Source = flattener.finish().into();

    let mut interpreter = PlaintextBackend::default();
    let new_simulator = Evaluator::from_messages(s.iter_messages(), &mut interpreter);

    assert_eq!(new_simulator.get_violations().len(), 0);

    Ok(())
}
