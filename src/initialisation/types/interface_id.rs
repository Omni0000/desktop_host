
use crate::capnp::common::interface_capnp::interface_id ;



pub type InterfaceId = u64 ;

impl<'a> From<interface_id::Reader<'a>> for InterfaceId {
    fn from( reader: interface_id::Reader<'a> ) -> Self {
        reader.get_id()
    }
}
