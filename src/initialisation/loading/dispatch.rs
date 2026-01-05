use std::sync::RwLock ;
use thiserror::Error ;
use wasmtime::{ AsContextMut, StoreContextMut };
use wasmtime::component::Val ;

use crate::PluginId ;
use super::{ FunctionData, FunctionReturnType };
use super::{ PluginTree, Socket, PluginInstance, PluginContext, ResourceWrapper, InterfaceParseError, ResourceCreationError, ResourceReceiveError };



#[derive( Error, Debug )]
pub enum DispatchError {
    #[error( "Deadlock" )] Deadlock,
    #[error( "Wit parser error: {0}")] WitParserError( #[from] InterfaceParseError ),
    #[error( "Invalid Interface: {0}" )] InvalidInterface( String ),
    #[error( "Invalid Function: {0}" )] InvalidFunction( String ),
    #[error( "Missing Response" )] MissingResponse,
    #[error( "Runtime Exception" )] RuntimeException( wasmtime::Error ),
    #[error( "Invalid Argument LIst" )] InvalidArgumentList,
    #[error( "Resource Create Error: {0}" )] ResourceCreationError( #[from] ResourceCreationError ),
    #[error( "Resource Receive Error: {0}" )] ResourceReceiveError( #[from] ResourceReceiveError ),
}
impl Into<Val> for DispatchError {
    fn into(self) -> Val { match self {
        Self::Deadlock => Val::Variant( "deadlock".to_string(), None ),
        Self::WitParserError( err ) => Val::Variant( "wit-parser-error".to_string(), Some( Box::new( Val::String( err.to_string() )))),
        Self::InvalidInterface( package ) => Val::Variant( "invalid-interface".to_string(), Some( Box::new( Val::String( package )))),
        Self::InvalidFunction( function ) => Val::Variant( "invalid-function".to_string(), Some( Box::new( Val::String( function )))),
        Self::MissingResponse => Val::Variant( "missing-response".to_string(), None ),
        Self::RuntimeException( exception ) => Val::Variant( "runtime-exception".to_string(), Some( Box::new( Val::String( exception.to_string() )))),
        Self::InvalidArgumentList => Val::Variant( "invalid-argument-list".to_string(), None ),
        Self::ResourceCreationError( err ) => err.into(),
        Self::ResourceReceiveError( err ) => err.into(),
    }}
}



impl PluginTree {
    pub fn dispatch_function_on_root(
        &self,
        interface_path: &str,
        function: &str,
        has_return: bool,
        data: &[Val],
    ) -> Socket<Result<Val, DispatchError>> {
        self.root_socket.dispatch_function( interface_path, function, has_return, data )
    }
}

impl Socket<RwLock<PluginInstance>> {

    pub fn dispatch_function(
        &self,
        interface_path: &str,
        function: &str,
        has_return: bool,
        data: &[Val],
    ) -> Socket<Result<Val, DispatchError>> {
        self.map(| plugin | plugin
            .write().map_err(|_| DispatchError::Deadlock )
            .and_then(| mut lock | lock.dispatch( interface_path, function, has_return, data ))
        )
    }

    pub fn dispatch_function_all(
        &self,
        mut ctx: StoreContextMut<PluginContext>,
        interface_path: &str,
        function: &FunctionData,
        data: &[Val],
    ) -> Val {
        debug_assert!( function.is_method() == false );
        self.map(| plugin | Val::Result( match Self::dispatch_function_of( &mut ctx, plugin, interface_path, function, data ) {
            Ok( val ) => Ok( Some( Box::new( val ))),
            Err( err ) => Err( Some( Box::new( err.into() ))),
        })).into()
    }

    pub fn dispatch_function_method(
        &self,
        ctx: StoreContextMut<PluginContext>,
        interface_path: &str,
        function: &FunctionData,
        data: &[Val],
    ) -> Val {
        debug_assert!( function.is_method() == true );
        Val::Result( match Self::route_method( self, ctx, interface_path, function, data ) {
            Ok( val ) => Ok( Some( Box::new( val ))),
            Err( err ) => Err( Some( Box::new( err.into() ))),
        })
    }

    #[inline] fn dispatch_function_of(
        ctx: &mut StoreContextMut<PluginContext>,
        plugin: &RwLock<PluginInstance>,
        interface_path: &str,
        function: &FunctionData,
        data: &[Val],
    ) -> Result<Val, DispatchError> {

        let mut lock = plugin.write().map_err(|_| DispatchError::Deadlock )?;
        let result = lock.dispatch( interface_path, function.name(), function.has_return(), data )?;
        
        Ok( match function.return_type() {
            FunctionReturnType::None | FunctionReturnType::DataNoResource => result,
            FunctionReturnType::DataWithResources => Self::wrap_resources( result, lock.id(), ctx )?,
        })
    }

    fn wrap_resources( val: Val, plugin_id: &PluginId, store: &mut impl AsContextMut ) -> Result<Val, ResourceCreationError> {
        Ok( match val {
            Val::Bool( _ )
            | Val::S8( _ ) | Val::S16( _ ) | Val::S32( _ ) | Val::S64( _ )
            | Val::U8( _ ) | Val::U16( _ ) | Val::U32( _ ) | Val::U64( _ )
            | Val::Float32( _ ) | Val::Float64( _ )
            | Val::Char( _ )
            | Val::String( _ )
            | Val::Enum( _ )
            | Val::Flags( _ )
            | Val::Variant( _, Option::None )
            | Val::Option( None )
            | Val::Result( Ok( Option::None )) | Val::Result( Err( Option::None )) => val,
            Val::List( list ) => Val::List( list.into_iter().map(| item | Self::wrap_resources( item, plugin_id, store )).collect::<Result<_,_>>()? ),
            Val::Record( entries ) => Val::Record( entries.into_iter().map(|( key, value )| Ok(( key, Self::wrap_resources( value, plugin_id, store )?)) ).collect::<Result<_,_>>()? ),
            Val::Tuple( list ) => Val::Tuple( list.into_iter().map(| item | Self::wrap_resources( item, plugin_id, store )).collect::<Result<_,_>>()? ),
            Val::Variant( variant, Some( data_box )) => Val::Variant( variant, Some( Box::new( Self::wrap_resources( *data_box, plugin_id, store )? ))),
            Val::Option( Some( data_box )) => Val::Option( Some( Box::new( Self::wrap_resources( *data_box, plugin_id, store )? ))),
            Val::Result( Ok( Some( data_box ))) => Val::Result( Ok( Some( Box::new( Self::wrap_resources( *data_box, plugin_id, store )? )))),
            Val::Result( Err( Some( data_box ))) => Val::Result( Err( Some( Box::new( Self::wrap_resources( *data_box, plugin_id, store )? )))),
            Val::Resource( handle ) => Val::Resource( ResourceWrapper::new( plugin_id.clone(), handle ).attach( store )? ),
            Val::Future( _ ) => unimplemented!( "'Val::Future' is not yet supported" ),
            Val::Stream( _ ) => unimplemented!( "'Val::Stream' is not yet supported" ),
            Val::ErrorContext( _ ) => unimplemented!( "'Val::ErrorContext' is not yet supported" ),
        })
    }

    #[inline] fn route_method(
        &self,
        mut ctx: StoreContextMut<PluginContext>,
        interface_path: &str,
        function: &FunctionData,
        data: &[Val],
    ) -> Result<Val, DispatchError> {

        let handle = match data.get(0) {
            Some( Val::Resource( handle )) => Ok( handle ),
            _ => Err( DispatchError::InvalidArgumentList ),
        }?;

        let resource = ResourceWrapper::from_handle( *handle, &mut ctx )?;
        let plugin = self.get( &resource.plugin_id )
            .map_err(|_| DispatchError::Deadlock )?
            .ok_or( DispatchError::InvalidArgumentList )?;

        let mut data = Vec::from( data );
        data[0] = Val::Resource( resource.resource_handle );

        Self::dispatch_function_of( &mut ctx, plugin, interface_path, function, &data )

    }

}

impl PluginInstance {

    const PLACEHOLDER_VAL: Val = Val::Tuple( vec![] );

    fn dispatch(
        &mut self,
        interface_path: &str,
        function: &str,
        returns: bool,
        data: &[Val],
    ) -> Result<Val, DispatchError> {
        
        let mut buffer = match returns {
            true => vec![ Self::PLACEHOLDER_VAL ],
            false => Vec::with_capacity( 0 ),
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

        Ok( match returns {
            true => buffer.pop().ok_or( DispatchError::MissingResponse )?,
            false => Self::PLACEHOLDER_VAL,
        })

    }
}
