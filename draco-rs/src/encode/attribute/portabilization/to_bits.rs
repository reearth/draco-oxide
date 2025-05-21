use crate::core::shared::DataValue;
use crate::core::shared::Vector;
use crate::encode::attribute::WritableFormat;
use crate::shared::attribute::Portable;

#[cfg(feature = "evaluation")]
use crate::eval;

use super::Config;
use super::PortabilizationImpl;

pub struct ToBits<Data>
    where Data: Vector

{
    /// iterator over the attribute values.
    /// this is not 'Vec<_>' because we want nicely consume the data.
    att_vals: std::vec::IntoIter<Data>,
}

impl<Data> ToBits<Data> 
    where 
        Data: Vector + Portable,
        Data::Component: DataValue
{
    pub fn new<F>(att_vals: Vec<Data>, _cfg: Config, writer: &mut F) -> Self 
        where F:FnMut((u8, u64)) 
    {
        #[cfg(feature = "evaluation")]
        eval::write_json_pair("portabilization", "ToBits".into(), writer);
        Self {
            att_vals: att_vals.into_iter(),
        }
    }
}

impl<Data> PortabilizationImpl for ToBits<Data> 
    where Data: Vector + Portable,
{
    fn portabilize(self) -> std::vec::IntoIter<WritableFormat> {
        self.att_vals.into_iter().map(|att_val| 
            WritableFormat::from(att_val)
        ).collect::<Vec<_>>().into_iter()
    }
}