
use crate::capnp::common_capnp::interface_id ;



pub type InterfaceId = String ;

impl<'a> TryFrom<interface_id::Reader<'a>> for InterfaceId {

    type Error = capnp::Error;

    fn try_from( reader: interface_id::Reader<'a> ) -> Result<Self, Self::Error> {
        Ok( reader.get_id()?.to_string()? )
    }

}