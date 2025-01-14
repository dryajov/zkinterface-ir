# Unreleased

Rust:
- gateset and features are now mandatory when creating a GateBuilder with new method (breaking change)
- add some checks for the validator to be in accordance with IR v1.0.1

# v3.0.0, 2022-04

Schema (.fbs):
- Add schema compilation in Makefile (`make fbs`)

Rust:
- Boolean example (`zki_sieve bool-example [--incorrect]`)
- Move to_r1cs converter from producers to consumers
- Add function (sub-circuit) into the builder with a FunctionBuilder
- Format the entire project with rustfmt
- Add SwitchBuilder
- When declaring a function, limit the use of Copy gates
- Add wirelist macro

# v2.0.0, 2021-12

Rust:
- Brand New API to evaluate SIEVE IR circuits. The circuit is now evaluated using a VM, each gate evaluation is 
delegated to a class implementing the `ZKBackend` trait.
- IR-to-IRsimple converter
- IR-to-R1CS converter
- R1CS-to-IR converter


# v1.0.0, 2021-08

Schema (.fbs):
- Switch / For / Functions statements.
- Iterator Expressions
- List and Range of wires

Rust:
- Structures, Validator and Evaluator adapted to the new schema.


# v0.2.0, 2021-04

Schema (.fbs):
- Directives GateInstance and GateWitness (@instance and @witness).
- Witness and instance values as ordered streams.
- GateFree (@free(wire)).

Rust:
- Structures, Validator and Evaluator adapted to the new schema.
- Improved Builder and Sink API.

# v0.1.1, 2021-03

Rust:
- Configurable field order (`zki_sieve example --field-order=101`)
- Example with an incorrect witness (`zki_sieve example --incorrect`)
- Rename library to zki_sieve

# v0.1.0, 2020-09
