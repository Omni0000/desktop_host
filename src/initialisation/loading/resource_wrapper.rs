use std::sync::{ Arc, Mutex };
use thiserror::Error ;
use wasmtime::component::{ Resource, ResourceAny, ResourceTable, Val };
use wasmtime::{ AsContextMut, StoreContextMut };

use crate::PluginId ;
use super::PluginContext ;



lazy_static::lazy_static! {
    static ref RESOURCE_TABLE: Mutex<ResourceTable> = Mutex::new( ResourceTable::new());
}

pub(super) struct ResourceWrapper {
    pub plugin_id: PluginId,
    pub resource_handle: ResourceAny,
}

#[derive( Debug, Error )]
pub enum ResourceCreationError {
    #[error( "Lock Rejected" )] LockRejected,
    #[error( "Resource Table Full" )] ResourceTableFull,
}
impl Into<Val> for ResourceCreationError {
    fn into( self ) -> Val { match self {
        Self::LockRejected => Val::Variant( "resource-table-lock-rejected".to_string(), None ),
        Self::ResourceTableFull => Val::Variant( "resource-table-full".to_string(), None ),
    }}
}

#[derive( Debug, Error )]
pub enum ResourceReceiveError {
    #[error( "Lock Rejected" )] LockRejected,
    #[error( "Invalid Handle" )] InvalidHandle,
}
impl Into<Val> for ResourceReceiveError {
    fn into( self ) -> Val { match self {
        Self::LockRejected => Val::Variant( "resource-table-lock-rejected".to_string(), None ),
        Self::InvalidHandle => Val::Variant( "invalid-resource-handle".to_string(), None ),
    }}
}

impl ResourceWrapper {
    pub fn new( plugin_id: PluginId, resource_handle: ResourceAny ) -> Self {
        Self { plugin_id, resource_handle }
    }
    pub fn attach(
        self,
        store: &mut impl AsContextMut,
    ) -> Result<ResourceAny, ResourceCreationError> {
        let mut lock = RESOURCE_TABLE.lock().map_err(|_| ResourceCreationError::LockRejected )?;
        let resource = lock.push( Arc::new( self )).map_err(|_| ResourceCreationError::ResourceTableFull )?;
        ResourceAny::try_from_resource( resource, store ).map_err(|_| unreachable!( "Resource already taken" ))
    }
    pub fn from_handle(
        handle: ResourceAny,
        mut store: &mut impl AsContextMut,
    ) -> Result<Arc<Self>, ResourceReceiveError> {
        let resource = Resource::try_from_resource_any( handle, &mut store ).map_err(|_| ResourceReceiveError::InvalidHandle )?;
        let lock = RESOURCE_TABLE.lock().map_err(|_| ResourceReceiveError::LockRejected )?;
        let wrapped = lock.get( &resource ).map_err(|_| ResourceReceiveError::InvalidHandle )?;
        // let resource = ResourceAny::try_from_resource( resource, &mut store ).map_err(|_| unreachable!( "Resource already taken" ))?;
        Ok( Arc::clone( wrapped ))
    }
    pub fn drop( _: StoreContextMut<PluginContext>, handle: u32) -> Result<(), wasmtime::Error> {
        let resource = Resource::<Arc<Self>>::new_own( handle );
        let mut lock = RESOURCE_TABLE.lock().map_err(|_| wasmtime::Error::new( ResourceReceiveError::LockRejected ))?;
        lock.delete( resource ).map_err(|_| wasmtime::Error::new( ResourceReceiveError::InvalidHandle ))?;
        Ok(())
    }
}
