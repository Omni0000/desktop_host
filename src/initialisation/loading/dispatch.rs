use std::sync::{ Arc, RwLock };
use thiserror::Error ;
use wasmtime::component::Val ;

use crate::initialisation::discovery::{ InterfaceParseError, RawInterfaceData };

use super::{ PluginTree, Socket, PluginInstance };



#[derive( Error, Debug )]
pub enum DispatchError {
    #[error( "Deadlock" )] Deadlock,
    #[error( "Wit parser error: {0}")] WitParserError( #[from] InterfaceParseError ),
    #[error( "Invalid Interface: {0}" )] InvalidInterface( String ),
    #[error( "Invalid Function: {0}" )] InvalidFunction( String ),
    #[error( "Missing Response" )] MissingResponse,
    #[error( "Runtime Exception" )] RuntimeException( wasmtime::Error ),
}
impl Into<Val> for DispatchError {
    fn into( self ) -> Val {
        match self {
            Self::Deadlock => Val::Variant( "deadlock".to_string(), None ),
            Self::WitParserError( err ) => Val::Variant( "wit-parser-error".to_string(), Some( Box::new( Val::String( err.to_string() )))),
            Self::InvalidInterface( package ) => Val::Variant( "invalid-interface".to_string(), Some( Box::new( Val::String( package )))),
            Self::InvalidFunction( function ) => Val::Variant( "invalid-function".to_string(), Some( Box::new( Val::String( function )))),
            Self::MissingResponse => Val::Variant( "missing-response".to_string(), None ),
            Self::RuntimeException( exception ) => Val::Variant( "runtime-exception".to_string(), Some( Box::new( Val::String( exception.to_string() )))),
        }
    }
}



impl PluginTree {
    pub fn dispatch_on_root(
        &self,
        interface_path: &str,
        function: &str,
        data: &[Val],
    ) -> Socket<Result<Val, DispatchError>> {

        self.root_socket.dispatch( interface_path, &self.root_interface, function, data )

    }
}

impl Socket<RwLock<PluginInstance>> {
    pub fn dispatch(
        &self,
        interface_path: &str,
        interface_data: &Arc<RawInterfaceData>,
        function: &str,
        data: &[Val],
    ) -> Socket<Result<Val, DispatchError>> {
        match self {
            Socket::AtMostOne( Option::None ) => Socket::AtMostOne( None ),
            Socket::AtMostOne( Some( plugin )) => Socket::AtMostOne( Some(
                dispatch( plugin, interface_path, interface_data, function, data )
            )),
            Socket::ExactlyOne( plugin ) => Socket::ExactlyOne(
                dispatch( plugin, interface_path, interface_data, function, data )
            ),
            Socket::AtLeastOne( plugins ) => Socket::AtLeastOne( plugins.iter()
                .map(| plugin | dispatch( plugin, interface_path, interface_data, function, data ))
                .collect()
            ),
            Socket::Any( plugins ) => Socket::Any( plugins.iter()
                .map(| plugin | dispatch( plugin, interface_path, interface_data, function, data ))
                .collect()
            ),
        }
    }
    pub fn dispatch_into_val(
        &self,
        interface_path: &str,
        interface_data: &Arc<RawInterfaceData>,
        function: &str,
        data: &[Val],
    ) -> Val {
        match self {
            Socket::AtMostOne( Option::None ) => Val::Option( None ),
            Socket::AtMostOne( Some( plugin )) => Val::Option( Some( Box::new(
                Self::into_val( dispatch( plugin, interface_path, interface_data, function, data ))
            ))),
            Socket::ExactlyOne( plugin ) => Self::into_val( dispatch( plugin, interface_path, interface_data, function, data )),
            Socket::AtLeastOne( plugins ) => Val::List( plugins.iter()
                .map(| plugin | Self::into_val( dispatch( plugin, interface_path, interface_data, function, data )))
                .collect()
            ),
            Socket::Any( plugins ) => Val::List( plugins.iter()
                .map(| plugin | Self::into_val( dispatch( plugin, interface_path, interface_data, function, data )))
                .collect()
            ),
        }
    }
    #[inline] fn into_val( result: Result<Val, DispatchError> ) -> Val {
        Val::Result( match result {
            Ok( data ) => Ok( Some( Box::new( data ))),
            Err( err ) => Err( Some( Box::new( err.into() ))),
        })
    }
}

#[inline] fn dispatch(
    plugin: &RwLock<PluginInstance>,
    interface_path: &str,
    interface_data: &Arc<RawInterfaceData>,
    function: &str,
    data: &[Val],
) -> Result<Val, DispatchError> {
    Ok( plugin.write().map_err(|_| DispatchError::Deadlock )?
        .dispatch( interface_path, interface_data, function, data )?
    )
}

const PLACEHOLDER_VAL: Val = Val::Tuple( vec![] );

impl PluginInstance {
    fn dispatch(
        &mut self,
        interface_path: &str,
        interface_data: &Arc<RawInterfaceData>,
        function: &str,
        data: &[Val],
    ) -> Result<Val, DispatchError> {
        
        let Some( return_type ) = interface_data.get_function_return_type( function ) else {
            return Err( DispatchError::InvalidFunction( format!( "{}:{}", interface_path, function )))};
        let mut buffer = match return_type {
            Option::None => Vec::with_capacity( 0 ),
            Option::Some { .. } => vec![ PLACEHOLDER_VAL ],
        };

        let interface_index = self.instance
            .get_export_index( &mut self.store, None, interface_path )
            .ok_or( DispatchError::InvalidInterface( interface_path.to_string() ))?;
        let func_index = self.instance
            .get_export_index( &mut self.store, Some( &interface_index ), function )
            .ok_or( DispatchError::InvalidFunction( format!( "{}:{}", interface_path, function )))?;
        let func = self.instance
            .get_func( &mut self.store, &func_index )
            .ok_or( DispatchError::InvalidFunction( format!( "{}:{}", interface_path, function )))?;
        func
            .call( &mut self.store, data, &mut buffer )
            .map_err(| err | DispatchError::RuntimeException( err ))?;
        let _ = func.post_return( &mut self.store );

        Ok( match return_type {
            Option::None => PLACEHOLDER_VAL,
            Some( _ ) => buffer.pop().ok_or( DispatchError::MissingResponse )?
        })

    }
}
