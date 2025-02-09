use crate::Result;
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::error::Error;
use std::io::Write;

use super::header::Header;
use crate::sieve_ir_generated::sieve_ir as g;
use crate::structs::value::{build_values_vector, try_from_values_vector, Value};

#[derive(Clone, Default, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Witness {
    pub header: Header,
    pub short_witness: Vec<Value>,
}

impl<'a> TryFrom<g::Witness<'a>> for Witness {
    type Error = Box<dyn Error>;

    /// Convert from Flatbuffers references to owned structure.
    fn try_from(g_witness: g::Witness) -> Result<Witness> {
        Ok(Witness {
            header: Header::try_from(g_witness.header())?,
            short_witness: try_from_values_vector(
                g_witness
                    .short_witness()
                    .ok_or_else(|| "Missing short_witness")?,
            )?,
        })
    }
}

impl<'a> TryFrom<&'a [u8]> for Witness {
    type Error = Box<dyn Error>;

    fn try_from(buffer: &'a [u8]) -> Result<Witness> {
        Witness::try_from(
            g::get_size_prefixed_root_as_root(&buffer)
                .message_as_witness()
                .ok_or_else(|| "Not a Witness message.")?,
        )
    }
}

impl Witness {
    /// Add this structure into a Flatbuffers message builder.
    pub fn build<'bldr: 'args, 'args: 'mut_bldr, 'mut_bldr>(
        &'args self,
        builder: &'mut_bldr mut FlatBufferBuilder<'bldr>,
    ) -> WIPOffset<g::Root<'bldr>> {
        let header = Some(self.header.build(builder));
        let short_witness = Some(build_values_vector(builder, &self.short_witness));

        let witness = g::Witness::create(
            builder,
            &g::WitnessArgs {
                header,
                short_witness,
            },
        );

        g::Root::create(
            builder,
            &g::RootArgs {
                message_type: g::Message::Witness,
                message: Some(witness.as_union_value()),
            },
        )
    }

    /// Writes this Witness as a Flatbuffers message into the provided buffer.
    ///
    /// # Examples
    /// ```
    /// use zki_sieve::Witness;
    /// use std::convert::TryFrom;
    ///
    /// let witness = Witness::default();
    /// let mut buf = Vec::<u8>::new();
    /// witness.write_into(&mut buf).unwrap();
    /// let witness2 = Witness::try_from(&buf[..]).unwrap();
    /// assert_eq!(witness, witness2);
    /// ```
    pub fn write_into(&self, writer: &mut impl Write) -> Result<()> {
        let mut builder = FlatBufferBuilder::new();
        let message = self.build(&mut builder);
        g::finish_size_prefixed_root_buffer(&mut builder, message);
        writer.write_all(builder.finished_data())?;
        Ok(())
    }
}
