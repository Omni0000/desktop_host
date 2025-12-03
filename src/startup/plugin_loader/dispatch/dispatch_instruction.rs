
use crate::startup::InterfaceId ;



#[derive( Debug )]
pub struct FunctionDispatchInstruction {
    pub(super) socket: InterfaceId,
    pub(super) function: String,
}

impl<'a> FunctionDispatchInstruction {
    pub fn new( socket: InterfaceId, function: String ) -> Self {
        Self { socket, function }
    }
}
